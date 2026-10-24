use std::{
   collections::HashSet,
   fmt,
   slice,
   str::FromStr,
};

use percent_encoding::{
   NON_ALPHANUMERIC,
   utf8_percent_encode,
};
use topcoat::{
   context::Cx,
   router::error::RouterErrorExt as _,
   runtime::{
      Event,
      shard,
      signal,
   },
   view::{
      View,
      ViewExt as _,
      component,
      view,
   },
};

use crate::{
   AppState,
   error::Error,
   routes::{
      helpers::{
         enrich_conversation,
         enrich_tweet_groups,
         request_prefs,
         state,
      },
      list::maybe_filter_retweets,
      timeline::{
         is_valid_username,
         multi_user_page,
         parse_usernames,
      },
   },
   types::{
      query::SearchProduct,
      timeline::{
         Chain,
         RankingMode,
         Tweets,
      },
      tweet::Tweet,
      user::User,
   },
   utils::formatters,
   views::{
      timeline::timeline_groups,
      tweet::{
         chain_items,
         tweet_item,
      },
      user_list::user_item,
   },
};

/// An account's timeline tab, linked by handle and fetched by id.
pub struct Account {
   pub username: String,
   pub user_id:  String,
}

/// A tweet, linked by its author's handle.
pub struct Post {
   pub username: String,
   pub id:       String,
}

/// A paginated list that "Load more" continues, in the text form the
/// browser sends back to the [`more`] shard.
pub enum Feed {
   Profile(String),
   Replies(Account),
   Media(Account),
   ProfileSearch {
      username: String,
      query:    String,
   },
   MultiUser(String),
   List {
      id:          String,
      no_retweets: bool,
   },
   ListMembers(String),
   Search {
      product: SearchProduct,
      query:   String,
   },
   Users(String),
   Conversation(Post, RankingMode),
   Retweets(Post),
   Quotes(Post),
}

#[derive(Debug)]
pub struct Malformed;

impl fmt::Display for Feed {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      match *self {
         Self::Profile(ref username) => write!(f, "profile/{username}"),
         Self::Replies(ref account) => {
            write!(f, "replies/{}/{}", account.username, account.user_id)
         },
         Self::Media(ref account) => write!(f, "media/{}/{}", account.username, account.user_id),
         Self::ProfileSearch {
            ref username,
            ref query,
         } => {
            write!(f, "profile-search/{username}/{query}")
         },
         Self::MultiUser(ref usernames) => write!(f, "multi/{usernames}"),
         Self::List {
            ref id,
            no_retweets,
         } => write!(f, "list/{id}/{no_retweets}"),
         Self::ListMembers(ref id) => write!(f, "members/{id}"),
         Self::Search { product, ref query } => write!(f, "search/{}/{query}", product.as_str()),
         Self::Users(ref query) => write!(f, "users/{query}"),
         Self::Conversation(ref post, sort) => {
            let sort = sort.sort_param().unwrap_or("relevance");
            write!(f, "conversation/{}/{}/{sort}", post.username, post.id)
         },
         Self::Retweets(ref post) => write!(f, "retweets/{}/{}", post.username, post.id),
         Self::Quotes(ref post) => write!(f, "quotes/{}/{}", post.username, post.id),
      }
   }
}

impl FromStr for Feed {
   type Err = Malformed;

   fn from_str(text: &str) -> Result<Self, Malformed> {
      fn pair(rest: &str) -> Result<(&str, &str), Malformed> {
         rest.split_once('/').ok_or(Malformed)
      }

      let username = |name: &str| {
         (is_valid_username(name) && !name.contains(','))
            .then(|| name.to_owned())
            .ok_or(Malformed)
      };
      let snowflake = |id: &str| {
         ((1..=19).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| id.to_owned())
            .ok_or(Malformed)
      };
      let account = |rest: &str| {
         let (name, id) = pair(rest)?;
         Ok::<_, Malformed>(Account {
            username: username(name)?,
            user_id:  snowflake(id)?,
         })
      };
      let post = |rest: &str| {
         let (name, id) = pair(rest)?;
         Ok::<_, Malformed>(Post {
            username: username(name)?,
            id:       snowflake(id)?,
         })
      };

      let (kind, rest) = pair(text)?;
      Ok(match kind {
         "profile" => Self::Profile(username(rest)?),
         "replies" => Self::Replies(account(rest)?),
         "media" => Self::Media(account(rest)?),
         "profile-search" => {
            let (name, query) = pair(rest)?;
            Self::ProfileSearch {
               username: username(name)?,
               query:    query.to_owned(),
            }
         },
         "multi" => {
            parse_usernames(rest)
               .filter(|names| names.len() <= 10)
               .ok_or(Malformed)?;
            Self::MultiUser(rest.to_owned())
         },
         "list" => {
            let (id, no_retweets) = pair(rest)?;
            Self::List {
               id:          snowflake(id)?,
               no_retweets: no_retweets.parse().map_err(|_| Malformed)?,
            }
         },
         "members" => Self::ListMembers(snowflake(rest)?),
         "search" => {
            let (product, query) = pair(rest)?;
            Self::Search {
               product: product.parse()?,
               query:   query.to_owned(),
            }
         },
         "users" => Self::Users(rest.to_owned()),
         "conversation" => {
            let (tweet, sort) = rest.rsplit_once('/').ok_or(Malformed)?;
            Self::Conversation(post(tweet)?, RankingMode::from_sort(Some(sort)))
         },
         "retweets" => Self::Retweets(post(rest)?),
         "quotes" => Self::Quotes(post(rest)?),
         _ => return Err(Malformed),
      })
   }
}

impl FromStr for SearchProduct {
   type Err = Malformed;

   fn from_str(text: &str) -> Result<Self, Malformed> {
      [Self::Top, Self::Latest, Self::Media, Self::People]
         .into_iter()
         .find(|product| product.as_str() == text)
         .ok_or(Malformed)
   }
}

enum Items {
   Groups(Vec<Tweets>),
   Tweets(Vec<Tweet>),
   Users(Vec<User>),
   Chains(Vec<Chain>),
}

impl Items {
   fn is_empty(&self) -> bool {
      match *self {
         Self::Groups(ref groups) => groups.iter().all(Vec::is_empty),
         Self::Tweets(ref tweets) => tweets.is_empty(),
         Self::Users(ref users) => users.is_empty(),
         Self::Chains(ref chains) => chains.iter().all(|chain| chain.content.is_empty()),
      }
   }

   /// Drop what earlier pages already showed, and X's repeats within this
   /// page, then add this page to `seen`.
   fn drop_seen(&mut self, seen: &mut HashSet<String>) {
      match *self {
         Self::Groups(ref mut groups) => {
            for group in &mut *groups {
               group.retain(|tweet| seen.insert(tweet.id.to_string()));
            }
            groups.retain(|group| !group.is_empty());
         },
         Self::Tweets(ref mut tweets) => tweets.retain(|tweet| seen.insert(tweet.id.to_string())),
         Self::Users(ref mut users) => users.retain(|user| seen.insert(user.id.clone())),
         Self::Chains(ref mut chains) => {
            chains.retain(|chain| {
               chain
                  .content
                  .first()
                  .is_none_or(|tweet| seen.insert(tweet.id.to_string()))
            });
         },
      }
   }
}

/// The ids a page shows, which later pages skip.
pub fn tweet_ids<'a, I>(tweets: I) -> Vec<String>
where
   I: IntoIterator<Item = &'a Tweet>,
{
   tweets
      .into_iter()
      .map(|tweet| tweet.id.to_string())
      .collect()
}

pub fn user_ids(users: &[User]) -> Vec<String> {
   users.iter().map(|user| user.id.clone()).collect()
}

pub fn chain_ids(chains: &[Chain]) -> Vec<String> {
   tweet_ids(chains.iter().filter_map(|chain| chain.content.first()))
}

impl Feed {
   /// The page this feed is the list of, for the link's no-JS fallback.
   fn page(&self) -> String {
      let encode = |query: &str| utf8_percent_encode(query, NON_ALPHANUMERIC).to_string();
      match *self {
         Self::Profile(ref username) => format!("/{username}"),
         Self::Replies(ref account) => format!("/{}/with_replies", account.username),
         Self::Media(ref account) => format!("/{}/media", account.username),
         Self::ProfileSearch {
            ref username,
            ref query,
         } if query.is_empty() => {
            format!("/{username}/search")
         },
         Self::ProfileSearch {
            ref username,
            ref query,
         } => {
            format!("/{username}/search?q={}", encode(query))
         },
         Self::MultiUser(ref usernames) => format!("/{usernames}"),
         Self::List {
            ref id,
            no_retweets: false,
         } => format!("/i/lists/{id}"),
         Self::List {
            ref id,
            no_retweets: true,
         } => format!("/i/lists/{id}?no_retweets=true"),
         Self::ListMembers(ref id) => format!("/i/lists/{id}/members"),
         Self::Search { product, ref query } => {
            let filter = match product {
               SearchProduct::Top => "&f=top",
               SearchProduct::Media => "&f=media",
               SearchProduct::Latest | SearchProduct::People => "",
            };
            format!("/search?q={}{filter}", encode(query))
         },
         Self::Users(ref query) => format!("/search?q={}&f=users", encode(query)),
         Self::Conversation(ref post, sort) => {
            let status = format!("/{}/status/{}", post.username, post.id);
            match sort.sort_param() {
               Some(param) => format!("{status}?sort={param}"),
               None => status,
            }
         },
         Self::Retweets(ref post) => format!("/{}/status/{}/retweets", post.username, post.id),
         Self::Quotes(ref post) => format!("/{}/status/{}/quotes", post.username, post.id),
      }
   }

   fn href(&self, cursor: &str) -> String {
      let href = formatters::cursor_url(&self.page(), cursor);
      if matches!(*self, Self::Conversation(..)) {
         format!("{href}#r")
      } else {
         href
      }
   }

   const fn label(&self) -> &'static str {
      if matches!(*self, Self::Conversation(..)) {
         "Load more replies"
      } else {
         "Load more"
      }
   }

   const fn has_footer(&self) -> bool {
      !matches!(
         *self,
         Self::ListMembers(_) | Self::Retweets(_) | Self::Conversation(..)
      )
   }

   async fn next_page(
      &self,
      state: &AppState,
      cursor: &str,
   ) -> Result<(Items, Option<String>), Error> {
      let api = &state.api;
      let cursor = Some(cursor);
      let timeline = match *self {
         Self::Profile(ref username) => {
            let mut tweets = api.get_profile(username, cursor).await?.tweets;
            for group in &mut tweets.content {
               group.retain(|tweet| !tweet.pinned);
            }
            tweets
         },
         Self::Replies(ref account) => {
            api.get_user_tweets_and_replies(&account.user_id, cursor)
               .await?
         },
         Self::Media(ref account) => api.get_user_media(&account.user_id, cursor).await?,
         Self::ProfileSearch {
            ref username,
            ref query,
         } => {
            let api_query = if query.is_empty() {
               format!("from:{username} include:nativeretweets")
            } else {
               format!("from:{username} {query}")
            };
            api.search(&api_query, cursor, SearchProduct::Latest)
               .await?
         },
         Self::MultiUser(ref usernames) => {
            let names = parse_usernames(usernames).unwrap_or_default();
            let (mut groups, next) = multi_user_page(state, &names, cursor).await;
            enrich_tweet_groups(state, &mut groups).await;
            return Ok((Items::Groups(groups), next));
         },
         Self::List {
            ref id,
            no_retweets,
         } => {
            let mut timeline = api.get_list_tweets(id, cursor).await?;
            timeline.content = maybe_filter_retweets(timeline.content, no_retweets);
            timeline
         },
         Self::ListMembers(ref id) => {
            let members = api.get_list_members(id, cursor).await?;
            return Ok((Items::Users(members.content), members.bottom));
         },
         Self::Search { product, ref query } => {
            let timeline = api.search(query, cursor, product).await?;
            let mut tweets = timeline.content.into_iter().flatten().collect::<Vec<_>>();
            enrich_tweet_groups(state, slice::from_mut(&mut tweets)).await;
            return Ok((Items::Tweets(tweets), timeline.bottom));
         },
         Self::Users(ref query) => {
            let users = api.search_users(query, cursor).await?;
            return Ok((Items::Users(users.content), users.bottom));
         },
         Self::Conversation(ref post, sort) => {
            let mut conversation = api.get_conversation(&post.id, cursor, sort).await?;
            enrich_conversation(state, &mut conversation).await;
            let replies = conversation.replies;
            return Ok((Items::Chains(replies.content), replies.bottom));
         },
         Self::Retweets(ref post) => {
            let users = api.get_retweeters(&post.id, cursor).await?;
            return Ok((Items::Users(users.content), users.bottom));
         },
         Self::Quotes(ref post) => {
            let query = format!("quoted_tweet_id:{}", post.id);
            api.search(&query, cursor, SearchProduct::Latest).await?
         },
      };
      let mut groups = timeline.content;
      enrich_tweet_groups(state, &mut groups).await;
      Ok((Items::Groups(groups), timeline.bottom))
   }
}

const MAX_RETRIES: usize = 3;

/// The end of a feed: a "Load more" shard while there is a cursor, then the
/// footer.
#[component]
pub async fn load_more<'a>(
   cx: &Cx,
   feed: &'a Feed,
   cursor: Option<&'a str>,
   seen: Vec<String>,
   #[default] attempt: usize,
) -> topcoat::Result<impl View> {
   // Boxed because the shard renders this component in turn.
   let next = cursor.filter(|cursor| !cursor.is_empty()).map(|cursor| {
      view! { cx =>
         more(feed: feed.to_string(), cursor: cursor.to_owned(), seen: seen, attempt: attempt)
      }
      .boxed()
   });

   Ok(view! {
      if let Some(next) = next {
         (next)
      } else if feed.has_footer() {
         <div class="timeline-footer"><h2 class="timeline-end">"No more items"</h2></div>
      }
   })
}

/// A "Load more" link that swaps itself for the next page on click, or when
/// it nears the viewport with infinite scroll on.
#[shard("/_topcoat/more")]
pub async fn more(
   cx: &Cx,
   feed: String,
   cursor: String,
   seen: Vec<String>,
   attempt: usize,
) -> topcoat::Result<impl View> {
   let feed = feed
      .parse::<Feed>()
      .ok()
      .ok_or_bad_request("invalid feed")?;
   let prefs = request_prefs(cx);
   let open = signal(cx, || false);
   let href = feed.href(&cursor);
   let label = feed.label();

   let mut seen = seen.into_iter().collect::<HashSet<_>>();
   let page = if open.get() {
      Some(
         feed
            .next_page(state(cx), &cursor)
            .await
            .inspect_err(|err| tracing::debug!("Loading more of {feed} failed: {err}"))
            .map(|(mut items, next)| {
               let continuation = (!items.is_empty()).then_some(next).flatten();
               items.drop_seen(&mut seen);
               (items, continuation)
            }),
      )
   } else {
      None
   };
   let seen = seen.into_iter().collect::<Vec<_>>();

   Ok(view! {
      match page {
         Some(Ok((items, continuation))) => {
            match items {
               Items::Groups(groups) => {
                  let group_refs = groups.iter().map(|group| group.iter().collect::<Vec<_>>()).collect::<Vec<_>>();
                  timeline_groups(groups: &group_refs, prefs: &prefs)
               },
               Items::Tweets(tweets) => {
                  #[key(tweet.id)]
                  for tweet in &tweets {
                     tweet_item(tweet: tweet, prefs: &prefs)
                  }
               },
               Items::Users(users) => {
                  for user in &users {
                     user_item(user: user, prefs: &prefs)
                  }
               },
               Items::Chains(chains) => chain_items(chains: &chains, prefs: &prefs),
            }
            load_more(feed: &feed, cursor: continuation.as_deref(), seen: seen)
         },
         Some(Err(_)) => {
            if attempt < MAX_RETRIES {
               load_more(feed: &feed, cursor: Some(cursor.as_str()), seen: seen, attempt: attempt + 1)
            } else {
               <div class="show-more"><a href=(&href)>"Error"</a></div>
            }
         },
         None => {
            <div class="show-more">
               <a
                  href=(&href)
                  @click=$(|event: Event| {
                     if event.ctrl_key {
                        return;
                     }
                     if event.meta_key {
                        return;
                     }
                     if event.shift_key {
                        return;
                     }
                     if event.alt_key {
                        return;
                     }
                     event.prevent_default();
                     open.set(true);
                  })
               >
                  $(if open.get() { "Loading..." } else { label })
               </a>
               if prefs.infinite_scroll {
                  <span class="scroll-trigger" @visible=$(|_e| open.set(true))></span>
               }
            </div>
         },
      }
   })
}
