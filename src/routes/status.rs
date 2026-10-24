#![expect(
   clippy::same_name_method,
   clippy::shadow_same,
   clippy::shadow_unrelated,
   reason = "topcoat's #[page] and view! expansions"
)]

use axum::{
   Router,
   extract::Path,
   response::{
      IntoResponse as _,
      Redirect,
      Response,
   },
   routing::get,
};
use serde::Deserialize;
use topcoat::{
   context::Cx,
   router::{
      StatusCode,
      error::see_other,
      page,
      path_param,
      query_params,
   },
   view::{
      BoxView,
      Child,
      View,
      ViewExt as _,
      class,
      component,
      view,
   },
};

use crate::{
   AppState,
   cache::{
      keys as cache_keys,
      ttl,
   },
   error::{
      Error,
      Result,
   },
   routes::helpers::{
      enrich_conversation,
      enrich_tweet_groups,
      get_cached_translation,
      request_prefs,
      state,
      user_agent,
   },
   types::{
      prefs::Prefs,
      query::SearchProduct,
      timeline::{
         Conversation,
         RankingMode,
      },
   },
   views::{
      document::document,
      embed::status_page,
      embed_activity::ActivityId,
      timeline::{
         timeline,
         to_top,
      },
      tweet::{
         reply_chains,
         thread_context,
         thread_context_bar,
         tweet_item,
      },
      user_list::user_list,
   },
};

const PREVIEW_AGENTS: [&str; 6] = [
   "Discordbot/",
   "Slackbot",
   "TelegramBot",
   "WhatsApp",
   "facebookexternalhit/",
   "Twitterbot/",
];

const SORT_MODES: [RankingMode; 3] = [
   RankingMode::Relevance,
   RankingMode::Recency,
   RankingMode::Likes,
];

#[query_params(error = bad_request)]
struct StatusQuery {
   cursor: Option<String>,
   scroll: Option<String>,
   sort:   Option<String>,
}

impl StatusQuery {
   fn is_scroll(&self) -> bool {
      self.scroll.as_deref() == Some("true")
   }
}

#[derive(Deserialize)]
struct StatusParams {
   username: String,
   id:       String,
   rest:     Option<String>,
}

path_param!(username);
path_param!(id);
path_param!(idx: usize, error = not_found);

pub fn router() -> Router<AppState> {
   Router::new()
      .route("/{username}/status/{id}/photo", get(status_redirect))
      .route("/{username}/status/{id}/video", get(status_redirect))
      .route("/{username}/status/{id}/video/{idx}", get(status_redirect))
      .route("/{username}/thread/{id}", get(status_redirect))
      .route("/{username}/statuses/{id}/{*rest}", get(statuses_redirect))
}

/// Redirect legacy media and thread URLs to the main status page.
async fn status_redirect(Path(params): Path<StatusParams>) -> Response {
   Redirect::to(&format!("/{}/status/{}", params.username, params.id)).into_response()
}

/// Every `statuses` subpage lives under `status`.
async fn statuses_redirect(Path(params): Path<StatusParams>) -> Response {
   let rest = params.rest.unwrap_or_default();
   Redirect::to(&format!("/{}/status/{}/{rest}", params.username, params.id)).into_response()
}

/// Fetch a conversation, caching the unsorted first page.
async fn fetch_conversation(
   state: &AppState,
   id: &str,
   cursor: Option<&str>,
   sort: RankingMode,
) -> Result<Conversation> {
   let cacheable = cursor.is_none() && sort == RankingMode::Relevance;
   let cache_key = cache_keys::conversation(id);
   if cacheable && let Some(cached) = state.cache.get(&cache_key) {
      tracing::debug!("Cache hit for conversation: {id}");
      return Ok(cached);
   }

   let mut conversation = state.api.get_conversation(id, cursor, sort).await?;
   state
      .api
      .resolve_unavailable_quote(&mut conversation.tweet)
      .await;
   if cacheable {
      state.cache.set(&cache_key, &conversation, ttl::DEFAULT);
   }
   Ok(conversation)
}

/// Auto-translate for embeds (OG tags / `ActivityPub` show translated text).
async fn auto_translate(state: &AppState, conversation: &mut Conversation) {
   if !conversation.tweet.is_translatable {
      return;
   }
   let kagi = &state.config.config.kagi_token;
   let token = (!kagi.is_empty()).then_some(kagi.as_str());
   if let Ok(translation) = get_cached_translation(state, &conversation.tweet, token).await
      && !translation.text.is_empty()
   {
      conversation.tweet.translation = Some(translation);
   }
}

#[page("/{username}/status/{id}")]
pub async fn status(cx: &Cx) -> topcoat::Result<impl View> {
   Box::pin(status_view(cx, None)).await
}

#[page("/{username}/statuses/{id}")]
pub async fn statuses(cx: &Cx) -> topcoat::Result<impl View> {
   Box::pin(status_view(cx, None)).await
}

/// Link unfurlers get the status page narrowed to the one photo, so the
/// preview shows that image instead of the whole tweet's media.
#[page("/{username}/status/{id}/photo/{idx}")]
pub async fn status_photo(cx: &Cx) -> topcoat::Result<impl View> {
   let agent = user_agent(cx);
   if !PREVIEW_AGENTS.iter().any(|preview| agent.contains(preview)) {
      let username = path_param::<Username>(cx);
      let id = path_param::<Id>(cx);
      return Err(see_other(format!("/{username}/status/{id}")).into());
   }
   let photo = path_param::<Idx>(cx)?.checked_sub(1);
   Box::pin(status_view(cx, photo)).await
}

async fn status_view(cx: &Cx, photo: Option<usize>) -> topcoat::Result<BoxView<'_>> {
   let state = state(cx);
   let username = path_param::<Username>(cx);
   let id = path_param::<Id>(cx);
   let query = query_params::<StatusQuery>(cx)?;
   let prefs = request_prefs(cx);
   let is_discord = user_agent(cx).contains("Discordbot");
   let sort = RankingMode::from_sort(query.sort.as_deref());
   let has_cursor = query.cursor.is_some();

   let mut conversation = fetch_conversation(state, id, query.cursor.as_deref(), sort).await?;
   enrich_conversation(state, &mut conversation).await;

   let focused_photo = photo.filter(|&idx| conversation.tweet.keep_only_photo(idx));
   let activity = is_discord.then(|| ActivityId::new(conversation.tweet.id, focused_photo));
   if !is_discord && !query.is_scroll() && !has_cursor {
      auto_translate(state, &mut conversation).await;
   }

   // Check this first because paginated responses omit the main tweet.
   if query.is_scroll() {
      let replies = conversation.replies;
      return Ok(view! { cx =>
         if replies.content.is_empty() {
            (StatusCode::NO_CONTENT)
         } else {
            reply_chains(
               chains: &replies.content,
               bottom_cursor: replies.bottom.as_deref().unwrap_or_default(),
               username: username,
               id: id,
               prefs: &prefs,
            )
         }
      }
      .boxed());
   }

   // Paginated requests with no replies indicate that the cursor reached a
   // dead end. Re-fetch the first page and render it without the bottom
   // cursor so "Load more replies" disappears.
   if has_cursor && conversation.replies.content.is_empty() {
      let mut first_page = fetch_conversation(state, id, None, sort).await?;
      first_page.replies.bottom = None;
      return Ok(view! { cx =>
         conversation_page(
            conversation: &first_page,
            has_cursor: false,
            username: username,
            id: id,
            prefs: &prefs,
            sort: sort,
            activity: activity,
         )
      }
      .boxed());
   }

   // Paginated responses (with cursor) don't include the main tweet.
   // Recover it from the first-page cache, or re-fetch without cursor.
   if conversation.tweet.id == 0
      && has_cursor
      && let Ok(first_page) = fetch_conversation(state, id, None, sort).await
   {
      conversation.tweet = first_page.tweet;
   }

   Ok(view! { cx =>
      conversation_page(
         conversation: &conversation,
         has_cursor: has_cursor,
         username: username,
         id: id,
         prefs: &prefs,
         sort: sort,
         activity: activity,
      )
   }
   .boxed())
}

#[page("/i/status/{id}")]
pub async fn status_by_id(cx: &Cx) -> topcoat::Result<impl View> {
   status_by_id_view(cx).await
}

#[page("/i/web/status/{id}")]
pub async fn web_status_by_id(cx: &Cx) -> topcoat::Result<impl View> {
   status_by_id_view(cx).await
}

/// Fetch the conversation directly and render the tweet page inline instead of
/// redirecting, matching X.com behaviour.
async fn status_by_id_view(cx: &Cx) -> topcoat::Result<impl View + '_> {
   let state = state(cx);
   let id = path_param::<Id>(cx);
   let query = query_params::<StatusQuery>(cx)?;
   let prefs = request_prefs(cx);
   let is_discord = user_agent(cx).contains("Discordbot");
   let sort = RankingMode::from_sort(query.sort.as_deref());

   let mut conversation = fetch_conversation(state, id, query.cursor.as_deref(), sort).await?;
   if !is_discord {
      auto_translate(state, &mut conversation).await;
   }
   let username = conversation.tweet.user.username.clone();
   let activity = is_discord.then(|| ActivityId::new(conversation.tweet.id, None));
   Ok(view! { cx =>
      conversation_page(
         conversation: &conversation,
         has_cursor: query.cursor.is_some(),
         username: &username,
         id: id,
         prefs: &prefs,
         sort: sort,
         activity: activity,
      )
   })
}

/// Render a conversation page (first page or paginated replies).
#[component]
async fn conversation_page<'a>(
   conversation: &'a Conversation,
   has_cursor: bool,
   username: &'a str,
   id: &'a str,
   prefs: &'a Prefs,
   sort: RankingMode,
   activity: Option<ActivityId>,
) -> topcoat::Result<impl View> {
   let tweet = &conversation.tweet;
   if !tweet.available && tweet.id == 0 {
      return Err(
         if tweet.tombstone.is_empty() {
            Error::TweetNotFound("Tweet is unavailable".to_owned())
         } else {
            Error::TweetTombstone(tweet.tombstone.clone())
         }
         .into(),
      );
   }

   let base = format!("/{username}/status/{id}");
   let has_replies = !prefs.hide_replies && !conversation.replies.content.is_empty();
   let replies_cursor = conversation.replies.bottom.as_deref().unwrap_or_default();
   let before = &conversation.before.content;
   let after = &conversation.after.content;
   let earlier = before
      .first()
      .filter(|first| tweet.thread_id != first.id && (first.reply_id > 0 || !first.available));
   let more_after = after
      .last()
      .filter(|_| conversation.after.has_more)
      .map(|last| {
         last
            .available
            .then(|| format!("/{}/status/{}#m", last.user.username, last.id))
      });

   Ok(view! {
      status_page(
         tweet: tweet,
         prefs: prefs,
         username: username,
         id: id,
         activity: activity,
         <div class="conversation">
            if has_cursor {
               <div class="main-thread">
                  <div class="main-tweet" id="m">tweet_item(tweet: tweet, prefs: prefs)</div>
               </div>
               <div class="timeline-item show-more"><a href=(format!("{base}#r"))>"Back to tweet"</a></div>
               thread_context_bar(tweet: tweet)
               <div class="replies" id="r">
                  reply_chains(
                     chains: &conversation.replies.content,
                     bottom_cursor: replies_cursor,
                     username: username,
                     id: id,
                     prefs: prefs,
                  )
               </div>
            } else {
               <div class="main-thread">
                  // Before context (parent tweets) with thread line
                  if !before.is_empty() {
                     <div class="before-tweet thread-line">
                        // "Earlier replies" link when thread doesn't start at root
                        if let Some(first) = earlier {
                           <div class="timeline-item more-replies earlier-replies">
                              <a
                                 class="more-replies-text"
                                 href=(format!("/{}/status/{}#m", first.user.username, first.id))
                              >
                                 "earlier replies"
                              </a>
                           </div>
                        }
                        #[key(reply.id)]
                        for (position, reply) in before.iter().enumerate() {
                           tweet_item(
                              tweet: reply,
                              prefs: prefs,
                              thread: thread_context(position, before.len(), false),
                           )
                        }
                     </div>
                  }
                  <div class="main-tweet" id="m">
                     tweet_item(
                        tweet: tweet,
                        prefs: prefs,
                        main: true,
                        extra_class: if after.is_empty() { "" } else { "thread thread-line" },
                        sort_toggle: (has_replies && !has_cursor)
                           .then(|| view! { sort_menu(base: &base, sort: sort) }.into()),
                     )
                  </div>
                  // After context (thread continuation) with thread line
                  if !after.is_empty() {
                     <div class="after-tweet thread-line">
                        #[key(reply.id)]
                        for (position, reply) in after.iter().enumerate() {
                           let is_last = position == after.len() - 1 && !conversation.after.has_more;
                           tweet_item(
                              tweet: reply,
                              prefs: prefs,
                              thread: thread_context(position, after.len(), is_last),
                           )
                        }
                        if let Some(href) = more_after {
                           <div class="timeline-item more-replies">
                              <a class="more-replies-text" href=(href)>"more replies"</a>
                           </div>
                        }
                     </div>
                  }
               </div>
               if has_replies {
                  thread_context_bar(tweet: tweet)
                  <div class="replies" id="r">
                     reply_chains(
                        chains: &conversation.replies.content,
                        bottom_cursor: replies_cursor,
                        username: username,
                        id: id,
                        prefs: prefs,
                     )
                  </div>
               }
            }
            to_top(focus: "#m")
         </div>
      )
   })
}

/// Lookup user by ID and redirect to their profile.
#[page("/i/user/{id}")]
pub async fn user_by_id(cx: &Cx) -> topcoat::Result<impl View> {
   let state = state(cx);
   let id = path_param::<Id>(cx);
   let cache_key = cache_keys::user_id(id);
   if let Some(username) = state.cache.get::<String>(&cache_key) {
      tracing::debug!("Cache hit for user ID: {id} -> {username}");
      return Err(see_other(format!("/{username}")).into());
   }

   let user = state.api.get_user_by_id(id).await?;
   // Cache the ID -> username mapping (long TTL since IDs don't change)
   state
      .cache
      .set(&cache_key, &user.username, ttl::USER_ID_MAPPING);
   Err::<(), _>(see_other(format!("/{}", user.username)).into())
}

#[component]
async fn sort_menu(base: &str, sort: RankingMode) -> topcoat::Result<impl View> {
   Ok(view! {
      <div class="reply-sort">
         <button class="reply-sort-btn" type="button">(sort.label()) " \u{25BE}"</button>
         <div class="reply-sort-menu">
            for mode in SORT_MODES {
               if mode == sort {
                  <span class="reply-sort-active">(mode.label())</span>
               } else if let Some(param) = mode.sort_param() {
                  <a href=(format!("{base}?sort={param}"))>(mode.label())</a>
               } else {
                  <a href=(base)>(mode.label())</a>
               }
            }
         </div>
      </div>
   })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Engagement {
   Retweets,
   Quotes,
}

/// Render the back-arrow + Retweets/Quotes tab bar above the list, or the
/// bare list for an infinite scroll request.
#[component]
async fn engagement_tabs<'a>(
   base: &'a str,
   active: Engagement,
   scroll: bool,
   child: Child<'a>,
) -> topcoat::Result<impl View> {
   Ok(view! {
      if scroll {
         (child)
      } else {
         <div class="timeline-container">
            <div class="engagement-header">
               <a class="back-arrow" href=(format!("{base}#m"))>"\u{2190}"</a>
               <ul class="tab">
                  <li class=(class!("tab-item", "active" if active == Engagement::Retweets))>
                     <a href=(format!("{base}/retweets"))>"Retweets"</a>
                  </li>
                  <li class=(class!("tab-item", "active" if active == Engagement::Quotes))>
                     <a href=(format!("{base}/quotes"))>"Quotes"</a>
                  </li>
               </ul>
            </div>
            (child)
         </div>
      }
   })
}

/// Render users who retweeted with infinite scrolling support.
#[page("/{username}/status/{id}/retweets")]
pub async fn retweets(cx: &Cx) -> topcoat::Result<impl View> {
   let state = state(cx);
   let username = path_param::<Username>(cx);
   let id = path_param::<Id>(cx);
   let query = query_params::<StatusQuery>(cx)?;
   let prefs = request_prefs(cx);

   let result = state
      .api
      .get_retweeters(id, query.cursor.as_deref())
      .await?;
   let base = format!("/{username}/status/{id}");
   let base_url = format!("{base}/retweets");
   let title = format!("Retweets - @{username}/status/{id}");

   Ok(view! {
      document(
         title: &title,
         prefs: &prefs,
         fragment: query.is_scroll(),
         engagement_tabs(
            base: &base,
            active: Engagement::Retweets,
            scroll: query.is_scroll(),
            user_list(
               users: &result.content,
               prefs: &prefs,
               cursor: result.bottom.as_deref(),
               base_url: &base_url,
            )
         )
      )
   })
}

/// Render tweets that quote this tweet with infinite scrolling support.
#[page("/{username}/status/{id}/quotes")]
pub async fn quotes(cx: &Cx) -> topcoat::Result<impl View> {
   let state = state(cx);
   let username = path_param::<Username>(cx);
   let id = path_param::<Id>(cx);
   let query = query_params::<StatusQuery>(cx)?;
   let prefs = request_prefs(cx);

   let results = state
      .api
      .search(
         &format!("quoted_tweet_id:{id}"),
         query.cursor.as_deref(),
         SearchProduct::Latest,
      )
      .await?;
   let mut groups = results.content;
   enrich_tweet_groups(state, &mut groups).await;
   let base = format!("/{username}/status/{id}");
   let base_url = format!("{base}/quotes");
   let title = format!("Quotes - @{username}/status/{id}");

   Ok(view! {
      document(
         title: &title,
         prefs: &prefs,
         fragment: query.is_scroll(),
         engagement_tabs(
            base: &base,
            active: Engagement::Quotes,
            scroll: query.is_scroll(),
            timeline(
               groups: &groups,
               prefs: &prefs,
               cursor: results.bottom.as_deref(),
               base_url: &base_url,
            )
         )
      )
   })
}

/// Edit history page handler.
#[page("/{username}/status/{id}/history")]
pub async fn edit_history(cx: &Cx) -> topcoat::Result<impl View> {
   let state = state(cx);
   let username = path_param::<Username>(cx);
   let id = path_param::<Id>(cx);
   let prefs = request_prefs(cx);

   let edits = state
      .api
      .get_edit_history(id)
      .await
      .map_err(|_| Error::NotFound("Tweet edit history not found".into()))?;
   let author = if edits.latest.user.username.is_empty() {
      username
   } else {
      &edits.latest.user.username
   };
   let title = format!(
      "Edit History: {} (@{author})",
      edits.latest.text.chars().take(40).collect::<String>(),
   );

   Ok(view! {
      document(
         title: &title,
         prefs: &prefs,
         <div class="edit-history">
            <div class="latest-edit">
               <div class="edit-history-header">"Latest post"</div>
               tweet_item(tweet: &edits.latest, prefs: &prefs)
            </div>
            if !edits.history.is_empty() {
               <div class="previous-edits">
                  <div class="edit-history-header">"Version history"</div>
                  #[key(tweet.id)]
                  for tweet in &edits.history {
                     <div class="tweet-edit">tweet_item(tweet: tweet, prefs: &prefs)</div>
                  }
               </div>
            }
         </div>
      )
   })
}
