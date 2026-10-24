#![expect(
   clippy::same_name_method,
   clippy::shadow_same,
   clippy::shadow_unrelated,
   reason = "topcoat's #[component] and view! expansions"
)]

use topcoat::{
   Result,
   context::Cx,
   view::{
      Child,
      View,
      class,
      component,
      view,
   },
};

use crate::{
   config::Config,
   types::{
      prefs::Prefs,
      timeline::Chain,
      tweet::{
         Card,
         CardKind,
         Entity,
         Gif,
         Photo,
         Poll,
         Tweet,
         TweetStats,
         Video,
      },
      user::User,
   },
   utils::{
      country,
      entity_expander::{
         expand_entities,
         expand_with_regex,
         html_escape,
      },
      formatters,
   },
   views::{
      document::strip_html,
      renderutils::{
         community_note,
         config,
         get_avatar_class,
         get_small_pic,
         icon,
         link_user,
         parse_location,
         proxied_image,
         trusted,
         tweet_link,
         verified_icon,
      },
   },
};

/// Thread context for visual thread line rendering.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ThreadContext {
   #[default]
   None,
   Start,
   Middle,
   End,
}

impl ThreadContext {
   const fn classes(self) -> Option<&'static str> {
      match self {
         Self::None => None,
         Self::Start => Some("thread thread-first"),
         Self::Middle => Some("thread thread-middle"),
         Self::End => Some("thread thread-last"),
      }
   }
}

/// Determine thread context based on position in a chain.
pub const fn thread_context(index: usize, total: usize, is_definite_end: bool) -> ThreadContext {
   if total == 1 {
      if is_definite_end {
         ThreadContext::End
      } else {
         ThreadContext::Middle
      }
   } else if index == 0 {
      ThreadContext::Start
   } else if index == total - 1 && is_definite_end {
      ThreadContext::End
   } else {
      ThreadContext::Middle
   }
}

/// Render one tweet, or its retweeted original under a retweet header.
#[component]
pub async fn tweet_item<'a>(
   cx: &Cx,
   tweet: &'a Tweet,
   prefs: &'a Prefs,
   #[default] main: bool,
   #[default] pinned: bool,
   #[default] thread: ThreadContext,
   #[default] extra_class: &'a str,
   #[default] index: usize,
   #[default] sort_toggle: Option<Child<'a>>,
) -> Result<impl View> {
   let config = config(cx);
   let (shown, retweeter) = tweet
      .retweet
      .as_deref()
      .map_or((tweet, None), |original| (original, Some(&tweet.user)));

   let status_link = format!("/{}/status/{}#m", shown.user.username, shown.id);
   let text_html = tweet_text_html(&shown.text, &shown.entities, &shown.location, config);
   let show_reply = !main
      && index == 0
      && thread == ThreadContext::None
      && !shown.reply.is_empty()
      && !is_reply_to_self(shown, pinned);
   let disclosures = [
      shown
         .paid_promotion
         .then(|| format!("{} Paid partnership", config.config.paid_emoji)),
      shown
         .ai_generated
         .then(|| format!("{} Made with AI", config.config.ai_emoji)),
   ]
   .into_iter()
   .flatten()
   .collect::<Vec<_>>();
   let latest_id = shown.history.iter().copied().max().unwrap_or(0);
   let has_edits = shown.history.len() > 1;

   Ok(view! {
      if !tweet.available {
         <div class="timeline-item unavailable" data-username=(&tweet.user.username)>
            <a class="unavailable-box" href=(format!("{}#m", tweet_link(tweet)))>
               (unavailable_text(tweet))
            </a>
            if let Some(quote) = tweet.quote.as_deref() {
               quote_item(quote: quote, prefs: prefs)
            }
         </div>
      } else {
         <div
            class=(class!(
               "timeline-item",
               format!("author-{}", shown.user.username),
               retweeter.map(|user| format!("retweet-{}", user.username)),
               extra_class,
               thread.classes(),
            ))
            data-username=(&shown.user.username)
         >
            if !main {
               <a class="tweet-link" href=(&status_link)></a>
            }
            <div class="tweet-body">
               <div>
                  if pinned {
                     <div class="pinned">
                        <span>icon(name: "pin", text: "Pinned Tweet")</span>
                     </div>
                  }
                  if let Some(user) = retweeter {
                     let retweet_label = format!("{} retweeted", user.fullname);
                     <div class="retweet-header">
                        <span>icon(name: "retweet", text: &retweet_label)</span>
                     </div>
                  }
                  <div class="tweet-header">
                     <a class="tweet-avatar" href=(format!("/{}", shown.user.username))>
                        <img
                           src=(formatters::get_pic_url(
                              &shown.user.user_pic,
                              config.config.base64_media,
                           ))
                           class=(get_avatar_class(prefs))
                           alt=""
                           loading="lazy"
                        >
                     </a>
                     <div class="tweet-name-row">
                        <div class="fullname-and-username">
                           link_user(user: &shown.user, class: "fullname")
                           verified_icon(user: &shown.user)
                           link_user(user: &shown.user, class: "username")
                           account_signal(user: &shown.user)
                        </div>
                        <span class="tweet-date">
                           if let Some(time) = shown.time {
                              <a href=(&status_link) title=(formatters::format_tweet_time(time))>
                                 (formatters::format_relative_time(time))
                              </a>
                           }
                        </span>
                     </div>
                  </div>
               </div>
               if show_reply {
                  replying_to(tweet: shown)
               }
               <div
                  class=(class!("tweet-content media-body", "tweet-bidi" if prefs.bidi_support))
                  dir="auto"
               >
                  (trusted(text_html))
               </div>
               if shown.is_translatable {
                  translate_button(id: shown.id)
               }
               if main {
                  for label in &disclosures {
                     <div class="disclosure-label">(label)</div>
                  }
               } else if !disclosures.is_empty() {
                  <div class="disclosure-label">(disclosures.join(" · "))</div>
               }
               if let Some(user) = shown.attribution.as_ref() {
                  <a class="attribution" href=(format!("/{}", user.username))>
                     mini_avatar(user: user, prefs: prefs)
                     <strong>(&user.fullname)</strong>
                     verified_icon(user: user)
                  </a>
               }
               if let Some(card) = shown.card.as_ref() {
                  card_preview(card: card, prefs: prefs)
               }
               if !shown.photos.is_empty() {
                  photo_gallery(photos: &shown.photos)
               }
               for video in shown.video.iter().chain(&shown.additional_videos) {
                  video_player(video: video, prefs: prefs)
               }
               for gif in &shown.gifs {
                  gif_player(gif: gif, autoplay: prefs.autoplay_gifs, poster: true)
               }
               if let Some(poll) = shown.poll.as_ref() {
                  poll_result(poll: poll)
               }
               if let Some(quote) = shown.quote.as_deref() {
                  quote_item(quote: quote, prefs: prefs)
               }
               community_note(note: shown.note.as_ref(), hidden: prefs.hide_community_notes)
               if main {
                  if let Some(time) = shown.time {
                     <p class="tweet-published">
                        if has_edits && shown.id == latest_id {
                           <a href=(format!("/{}/status/{}/history", shown.user.username, shown.id))>
                              "Last edited " (formatters::format_tweet_time(time))
                           </a>
                        } else {
                           (formatters::format_tweet_time(time))
                        }
                     </p>
                  }
                  if has_edits && shown.id != latest_id {
                     <div class="latest-post-version">
                        "There's a new version of this post. "
                        <a href=(format!("/{}/status/{latest_id}#m", shown.user.username))>
                           "See the latest post"
                        </a>
                     </div>
                  }
               }
               if !shown.media_tags.is_empty() {
                  <div class="media-tag-block">
                     icon(name: "user")
                     for (position, user) in shown.media_tags.iter().enumerate() {
                        if position > 0 {
                           ", "
                        }
                        <a class="media-tag" href=(format!("/{}", user.username)) title=(&user.username)>
                           (&user.fullname)
                        </a>
                     }
                  </div>
               }
               if !prefs.hide_tweet_stats {
                  tweet_stats(stats: &shown.stats, username: &shown.user.username, id: shown.id, extra: sort_toggle)
               } else if let Some(toggle) = sort_toggle {
                  <div class="tweet-stats">(toggle)</div>
               }
            </div>
         </div>
      }
   })
}

fn unavailable_text(tweet: &Tweet) -> &str {
   if !tweet.tombstone.is_empty() {
      &tweet.tombstone
   } else if !tweet.text.is_empty() {
      &tweet.text
   } else {
      "This tweet is unavailable"
   }
}

#[component]
async fn translate_button(id: i64) -> Result<impl View> {
   Ok(view! {
      <button
         class="translate-btn"
         hx-get=(format!("/translate/{id}"))
         hx-swap="outerHTML"
         hx-indicator="closest .translate-btn"
      >
         "Translate post"
      </button>
   })
}

/// Lay photos out in one row, or two rows once there are three or more.
#[component]
async fn photo_gallery(cx: &Cx, photos: &[Photo]) -> Result<impl View> {
   let config = config(cx);
   let rows = if photos.len() < 3 {
      vec![photos]
   } else {
      let (top, bottom) = photos.split_at(photos.len().div_ceil(2));
      vec![top, bottom]
   };

   Ok(view! {
      <div class="attachments">
         for (row_index, row) in rows.into_iter().enumerate() {
            <div class="gallery-row" style=((row_index > 0).then_some("margin-top: .25em"))>
               for photo in row {
                  let small = if photo.url.contains("name=") {
                     formatters::get_pic_url(&photo.url, config.config.base64_media)
                  } else {
                     get_small_pic(&photo.url, config)
                  };
                  <div class="attachment image">
                     <a
                        class="still-image"
                        href=(formatters::get_orig_pic_url(&photo.url, config.config.base64_media))
                        target="_blank"
                     >
                        <img src=(small) alt=(&photo.alt_text) loading="lazy">
                     </a>
                     if !photo.alt_text.is_empty() {
                        <p class="alt-text">"ALT  " (&photo.alt_text)</p>
                     }
                  </div>
               }
            </div>
         }
      </div>
   })
}

/// A tweet stays unlabelled when it only replies to its own author.
fn is_reply_to_self(tweet: &Tweet, pinned: bool) -> bool {
   !pinned && tweet.reply.len() == 1 && tweet.reply[0].eq_ignore_ascii_case(&tweet.user.username)
}

#[component]
async fn replying_to(tweet: &Tweet) -> Result<impl View> {
   Ok(view! {
      <div class="replying-to">
         "Replying to "
         for (position, username) in tweet.reply.iter().enumerate() {
            if position > 0 {
               " "
            }
            <a href=(format!("/{username}"))>"@" (username)</a>
         }
      </div>
   })
}

#[component]
async fn mini_avatar(cx: &Cx, user: &User, prefs: &Prefs) -> Result<impl View> {
   let pic = formatters::get_user_pic(&user.user_pic, "_mini");
   Ok(view! {
      <img
         src=(formatters::get_pic_url(&pic, config(cx).config.base64_media))
         class=(class!("avatar", "round" if !prefs.square_avatars, "mini"))
         alt=""
         loading="lazy"
      >
   })
}

#[component]
async fn gif_player(cx: &Cx, gif: &Gif, autoplay: bool, poster: bool) -> Result<impl View> {
   let config = config(cx);
   let src = formatters::get_vid_url(
      &gif.url,
      &config.config.hmac_key,
      config.config.base64_media,
   );
   Ok(view! {
      <div class="attachments media-gif">
         <div class="gallery-gif" style="max-height: unset">
            <div class="attachment">
               if poster {
                  <video
                     class="gif"
                     poster=(get_small_pic(&gif.thumb, config))
                     autoplay=(autoplay)
                     controls=""
                     muted=""
                     loop=""
                  >
                     <source src=(src) type="video/mp4">
                  </video>
               } else {
                  <video autoplay="" loop="" muted="" playsinline="">
                     <source src=(src) type="video/mp4">
                  </video>
               }
            </div>
         </div>
      </div>
   })
}

#[component]
async fn quote_item(cx: &Cx, quote: &Tweet, prefs: &Prefs) -> Result<impl View> {
   let config = config(cx);
   let user = if quote.user.username.is_empty() {
      "i"
   } else {
      &quote.user.username
   };
   let quote_link = format!("/{}/status/{}", quote.user.username, quote.id);
   let text_html = tweet_text_html(&quote.text, &quote.entities, "", config);
   let outdated =
      quote.history.len() > 1 && quote.id != quote.history.iter().copied().max().unwrap_or(0);

   Ok(view! {
      if !quote.available {
         <div class="quote unavailable">
            <div class="unavailable-quote">
               if quote.tombstone.is_empty() && quote.text.is_empty() {
                  "This quoted post is unavailable."
               } else {
                  (unavailable_text(quote))
               }
               if quote.id != 0 {
                  <div class="unavailable-actions">
                     <a href=(format!("/{user}/status/{}", quote.id))>"Try viewing anyway"</a>
                     " · "
                     <a href=(format!("https://x.com/{user}/status/{}", quote.id))>"View on X"</a>
                  </div>
               }
            </div>
         </div>
      } else {
         <div class="quote quote-big">
            <a class="quote-link" href=(&quote_link)></a>
            <div class="tweet-name-row">
               <div class="fullname-and-username">
                  mini_avatar(user: &quote.user, prefs: prefs)
                  link_user(user: &quote.user, class: "fullname")
                  verified_icon(user: &quote.user)
                  link_user(user: &quote.user, class: "username")
                  account_signal(user: &quote.user)
               </div>
               <span class="tweet-date">
                  if let Some(time) = quote.time {
                     <a href=(&quote_link) title=(formatters::format_tweet_time(time))>
                        (formatters::format_relative_time(time))
                     </a>
                  }
               </span>
            </div>
            if !quote.reply.is_empty() {
               replying_to(tweet: quote)
            }
            if !quote.text.is_empty() {
               <div class="quote-text" dir="auto">(trusted(text_html))</div>
               if quote.is_translatable {
                  translate_button(id: quote.id)
               }
            }
            if let Some(card) = quote.card.as_ref() {
               card_preview(card: card, prefs: prefs)
            }
            if !quote.photos.is_empty() {
               photo_gallery(photos: &quote.photos)
            }
            for video in quote.video.iter().chain(&quote.additional_videos) {
               video_player(video: video, prefs: prefs)
            }
            for gif in &quote.gifs {
               gif_player(gif: gif, autoplay: true, poster: false)
            }
            if let Some(poll) = quote.poll.as_ref() {
               poll_result(poll: poll)
            }
            community_note(note: quote.note.as_ref(), hidden: prefs.hide_community_notes)
            if quote.has_thread {
               <a class="show-thread" href=(&quote_link)>"Show this thread"</a>
            }
            if outdated {
               <div class="quote-latest">"There's a new version of this post"</div>
            }
         </div>
      }
   })
}

#[component]
async fn poll_result(poll: &Poll) -> Result<impl View> {
   #[expect(clippy::cast_precision_loss, reason = "poll vote counts fit in f64")]
   let total_votes = poll.votes.max(1) as f64;
   let leader = usize::try_from(poll.leader).ok();
   let options = poll
      .options
      .iter()
      .enumerate()
      .map(move |(position, option)| {
         #[expect(clippy::cast_precision_loss, reason = "poll vote counts fit in f64")]
         let votes = poll.values.get(position).copied().unwrap_or(0).max(0) as f64;
         let percent = format!("{:.0}%", votes / total_votes * 100.0);
         (option, percent, leader == Some(position))
      });

   Ok(view! {
      <div class="poll">
         if !poll.choice_images.is_empty() {
            <div class="poll-choice-images">
               for image in &poll.choice_images {
                  proxied_image(url: image, class: "poll-choice-img")
               }
            </div>
         }
         for (option, percent, is_leader) in options {
            <div class=(class!("poll-meter", "leader" if is_leader))>
               <span class="poll-choice-bar" style=(format!("width: {percent}"))></span>
               <span class="poll-choice-value">(&percent)</span>
               <span class="poll-choice-option">(option)</span>
            </div>
         }
         <span class="poll-info">
            (formatters::format_with_commas(poll.votes)) " votes \u{2022} " (poll.status_text())
         </span>
      </div>
   })
}

#[component]
async fn card_content(card: &Card) -> Result<impl View> {
   Ok(view! {
      <div class="card-content">
         <h2 class="card-title">(&card.title)</h2>
         if !card.text.is_empty() {
            <p class="card-description">(&card.text)</p>
         }
         if card.member_count > 0 && !card.dest.is_empty() {
            <span class="card-destination">
               (format!("{} · {} Members", card.dest, card.member_count))
            </span>
         } else if !card.dest.is_empty() {
            <span class="card-destination">(&card.dest)</span>
         }
      </div>
   })
}

/// Render a link preview. App, Player, Summary and `StoreLink` cards are
/// small, every other kind is large.
#[component]
async fn card_preview(card: &Card, prefs: &Prefs) -> Result<impl View> {
   let hidden = matches!(card.kind, CardKind::Hidden | CardKind::Unknown);
   let large = !matches!(
      card.kind,
      CardKind::App | CardKind::Player | CardKind::Summary | CardKind::StoreLink
   );

   Ok(view! {
      if !hidden {
         <div class=(class!("card", "large" if large))>
            if let Some(video) = card.video.as_ref() {
               <div class="card-container">
                  video_player(video: video, prefs: prefs)
                  <a class="card-content-container" href=(&card.url)>
                     card_content(card: card)
                  </a>
               </div>
            } else {
               <a class="card-container" href=(&card.url)>
                  if !card.image.is_empty() {
                     <div class="card-image-container">
                        <div class="card-image">
                           proxied_image(url: &card.image)
                           if matches!(card.kind, CardKind::Player) {
                              <div class="card-overlay">
                                 <div class="overlay-circle">
                                    <span class="overlay-triangle"></span>
                                 </div>
                              </div>
                           }
                        </div>
                     </div>
                  }
                  <div class="card-content-container">card_content(card: card)</div>
               </a>
            }
         </div>
      }
   })
}

/// Sticky reminder of the post the visible replies belong to, revealed by
/// `threadContext.js` once the opening thread scrolls past.
#[component]
pub async fn thread_context_bar(cx: &Cx, tweet: &Tweet) -> Result<impl View> {
   let config = config(cx);
   let shown = tweet.retweet.as_deref().unwrap_or(tweet);
   let text = strip_html(&tweet_text_html(&shown.text, &shown.entities, "", config));
   let avatar_url = formatters::get_pic_url(&shown.user.user_pic, config.config.base64_media);

   Ok(view! {
      if !shown.text.trim().is_empty() {
         <aside class="thread-context-bar" aria-label="Original post context">
            <a class="thread-context-link" href="#m" title="Back to the original post">
               <span class="sr-only">"Back to the original post: "</span>
               <img class="thread-context-avatar" src=(avatar_url) alt="" loading="lazy">
               <span class="thread-context-copy">
                  <strong class="thread-context-author">(&shown.user.fullname)</strong>
                  <span class="thread-context-text" dir="auto">(text)</span>
               </span>
            </a>
         </aside>
      }
   })
}

/// Inline country marker beside a poster's name, expanding to the same detail
/// the profile panel shows.
#[component]
async fn account_signal(user: &User) -> Result<impl View> {
   let country = country::display_name(&user.account_based_in);
   let maybe_proxied = user.location_accurate == Some(false);

   Ok(view! {
      if !user.account_based_in.is_empty() {
         <details class="tweet-account-trigger">
            <summary
               class="tweet-account-location"
               title=(format!("Show X-reported account information: {country}"))
            >
               if let Some(flag) = country::flag(country) {
                  <span class="account-signal-flag" role="img" aria-label=(country)>(flag.to_string())</span>
               } else {
                  <span class="icon-location" role="img" aria-label=(country)></span>
               }
               if maybe_proxied {
                  <span
                     class="tweet-account-warning"
                     title="X says this location may be affected by a proxy or VPN"
                  >
                     "!"
                  </span>
               }
            </summary>
            <div class="tweet-account-popover">
               <strong>(country)</strong>
               if !user.connection_source.is_empty() {
                  <span>"Connected via " (&user.connection_source)</span>
               }
               if maybe_proxied {
                  <span class="tweet-account-popover-warning">
                     "Location may be affected by a proxy or VPN."
                  </span>
               }
               <small>"Account-level information, not a live location or per-post device."</small>
            </div>
         </details>
      }
   })
}

/// Expand a tweet's entities and append its location, as trusted HTML.
fn tweet_text_html(text: &str, entities: &[Entity], location: &str, config: &Config) -> String {
   let processed = if entities.is_empty() {
      expand_with_regex(text)
   } else {
      expand_entities(text, entities)
   };

   let mut result = formatters::replace_urls(&processed, config);
   if !location.is_empty() {
      result.push_str(&location_html(location));
   }
   result
}

#[component]
async fn video_player(cx: &Cx, video: &Video, prefs: &Prefs) -> Result<impl View> {
   let config = config(cx);
   let has_card_content = !video.description.is_empty() || !video.title.is_empty();
   let thumb = if video.thumb.is_empty() {
      String::new()
   } else {
      get_small_pic(&video.thumb, config)
   };
   let mp4_src = video
      .best_mp4_url()
      .map(|url| formatters::get_vid_url(url, &config.config.hmac_key, config.config.base64_media));

   Ok(view! {
      <div class="attachments card">
         <div class=(class!("gallery-video", "card-container" if has_card_content))>
            <div class="attachment video-container">
               if !video.available {
                  <img src=(&thumb) loading="lazy">
                  <div class="video-overlay">
                     if video.reason == "dmcaed" {
                        <p>"This media has been disabled in response to a report by the copyright owner"</p>
                     } else {
                        <p>"This media is unavailable"</p>
                     }
                  </div>
               } else if !prefs.mp4_playback {
                  <img src=(&thumb) loading="lazy">
                  <div class="video-overlay">
                     <form method="post" action="/enablemp4">
                        <button type="submit">"Enable mp4 playback"</button>
                     </form>
                  </div>
               } else if let Some(src) = mp4_src {
                  <video poster=(&thumb) controls="" muted=(prefs.mute_videos)>
                     <source src=(src) type="video/mp4">
                  </video>
               } else {
                  <img src=(&thumb) loading="lazy">
                  <div class="video-overlay">
                     <div class="overlay-duration">(formatters::format_duration(video.duration_ms))</div>
                  </div>
               }
            </div>
            if has_card_content {
               <div class="card-content">
                  <h2 class="card-title">(&video.title)</h2>
                  if !video.description.is_empty() {
                     <p class="card-description">(&video.description)</p>
                  }
               </div>
            }
         </div>
      </div>
   })
}

/// Retweet counts link to the retweeters page and quote counts to their
/// search. `extra` lands after the counts, for the main tweet's reply sort.
#[component]
async fn tweet_stats<'a>(
   stats: &'a TweetStats,
   username: &'a str,
   id: i64,
   extra: Option<Child<'a>>,
) -> Result<impl View> {
   let count = |value: i64| {
      if value > 0 {
         formatters::format_with_commas(value)
      } else {
         String::new()
      }
   };

   let [replies, retweets, quotes, likes, views] = [
      stats.replies,
      stats.retweets,
      stats.quotes,
      stats.likes,
      stats.views,
   ]
   .map(count);

   Ok(view! {
      <div class="tweet-stats">
         <span class="tweet-stat">icon(name: "comment", text: &replies)</span>
         <a class="tweet-stat" href=(format!("/{username}/status/{id}/retweets")) title="Retweets">
            icon(name: "retweet", text: &retweets)
         </a>
         if stats.quotes > 0 {
            <a class="tweet-stat" href=(format!("/{username}/status/{id}/quotes")) title="Quotes">
               icon(name: "quote", text: &quotes)
            </a>
         }
         <span class="tweet-stat">icon(name: "heart", text: &likes)</span>
         <span class="tweet-stat">icon(name: "views", text: &views)</span>
         if let Some(extra) = extra {
            (extra)
         }
      </div>
   })
}

fn location_html(location: &str) -> String {
   let (place, url) = parse_location(location);
   let place = html_escape(&place);
   if url.is_empty() {
      format!(r#"<span class="tweet-geo"> – at {place}</span>"#)
   } else {
      let url = html_escape(&url);
      format!(r#"<span class="tweet-geo"> – at <a href="{url}">{place}</a></span>"#)
   }
}

/// Render reply chains with thread lines and a "Load more replies" link.
#[component]
pub async fn reply_chains<'a>(
   chains: &'a [Chain],
   bottom_cursor: &'a str,
   username: &'a str,
   id: &'a str,
   prefs: &'a Prefs,
) -> Result<impl View> {
   let chains = chains.iter().filter(|chain| !chain.content.is_empty());

   Ok(view! {
      for chain in chains {
         let total = chain.content.len();
         <div class="reply thread thread-line">
            for (position, tweet) in chain.content.iter().enumerate() {
               let is_last = position == total - 1 && !chain.has_more;
               tweet_item(
                  tweet: tweet,
                  prefs: prefs,
                  thread: thread_context(position, total, is_last),
               )
            }
            if chain.has_more {
               if let Some(last) = chain.content.last() {
                  <div class="timeline-item more-replies">
                     <a
                        class="more-replies-text"
                        href=(last.available.then(|| format!("/{}/status/{}#m", last.user.username, last.id)))
                     >
                        "View more replies"
                     </a>
                  </div>
               }
            }
         </div>
      }
      if !bottom_cursor.is_empty() {
         <div class="show-more">
            <a href=(format!("{}#r", formatters::cursor_url(&format!("/{username}/status/{id}"), bottom_cursor)))>
               "Load more replies"
            </a>
         </div>
      }
   })
}
