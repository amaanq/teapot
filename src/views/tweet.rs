use maud::{
   Markup,
   PreEscaped,
   html,
};

use crate::{
   config::Config,
   types::{
      Card,
      CardKind,
      Chain,
      Entity,
      Photo,
      Poll,
      Prefs,
      Tweet,
      TweetStats,
      User,
      Video,
   },
   utils::{
      entity_expander::expand_with_regex,
      expand_entities,
      formatters,
   },
   views::renderutils::{
      button_referer,
      gen_img,
      get_avatar_class,
      get_small_pic,
      icon,
      link_user,
      parse_location,
      render_community_note,
      verified_icon,
   },
};

/// Get the link to a tweet (with `#m` anchor for scroll target).
fn get_link(tweet: &Tweet) -> String {
   let link = super::renderutils::tweet_link(tweet);
   if link.is_empty() {
      link
   } else {
      format!("{link}#m")
   }
}

/// Thread context for visual thread line rendering.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThreadContext {
   None,
   Start,
   Middle,
   End,
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

/// Builder for rendering a tweet with configurable options.
#[expect(
   clippy::module_name_repetitions,
   reason = "TweetRenderer is the canonical name used across the codebase"
)]
pub struct TweetRenderer<'a> {
   tweet:       &'a Tweet,
   config:      &'a Config,
   is_main:     bool,
   pinned:      bool,
   prefs:       Option<&'a Prefs>,
   thread_ctx:  ThreadContext,
   extra_class: &'a str,
   index:       usize,
   sort_toggle: Option<&'a Markup>,
}

impl<'a> TweetRenderer<'a> {
   pub const fn new(tweet: &'a Tweet, config: &'a Config, is_main: bool) -> Self {
      Self {
         tweet,
         config,
         is_main,
         pinned: false,
         prefs: None,
         thread_ctx: ThreadContext::None,
         extra_class: "",
         index: 0,
         sort_toggle: None,
      }
   }

   pub const fn pinned(mut self, pinned: bool) -> Self {
      self.pinned = pinned;
      self
   }

   pub const fn prefs(mut self, prefs: &'a Prefs) -> Self {
      self.prefs = Some(prefs);
      self
   }

   pub const fn maybe_prefs(mut self, prefs: Option<&'a Prefs>) -> Self {
      self.prefs = prefs;
      self
   }

   pub const fn thread_ctx(mut self, ctx: ThreadContext) -> Self {
      self.thread_ctx = ctx;
      self
   }

   pub const fn extra_class(mut self, class: &'a str) -> Self {
      self.extra_class = class;
      self
   }

   pub const fn index(mut self, index: usize) -> Self {
      self.index = index;
      self
   }

   pub const fn sort_toggle(mut self, toggle: Option<&'a Markup>) -> Self {
      self.sort_toggle = toggle;
      self
   }

   #[expect(
      clippy::cognitive_complexity,
      reason = "maud HTML template with many conditional branches"
   )]
   pub fn render(&self) -> Markup {
      let tweet = self.tweet;
      let config = self.config;
      let is_main = self.is_main;
      let pinned = self.pinned;
      let prefs = self.prefs;
      let thread_ctx = self.thread_ctx;
      let extra_class = self.extra_class;
      let index = self.index;

      // Handle unavailable tweets (tombstoned) — use <a> to make clickable
      if !tweet.available {
         let link = get_link(tweet);
         let text = if !tweet.tombstone.is_empty() {
            &tweet.tombstone
         } else if !tweet.text.is_empty() {
            &tweet.text
         } else {
            "This tweet is unavailable"
         };
         return html! {
             div class="timeline-item unavailable" data-username=(tweet.user.username) {
                 a class="unavailable-box" href=(link) { (text) }
                 @if let Some(ref quote) = tweet.quote {
                     (render_quote(quote, config, prefs))
                 }
             }
         };
      }

      // Handle retweets: extract the inner tweet and show retweet header
      let (display_tweet, retweet_by) = tweet
         .retweet
         .as_ref()
         .map_or((tweet, None), |rt| (rt.as_ref(), Some(&tweet.user)));

      // Build CSS class based on context.
      // Only "thread-last" for the final tweet; no start/middle classes.
      // Includes .author-{user} and .retweet-{user} for adblock filtering.
      let author_class = format!("author-{}", display_tweet.user.username);
      let retweet_class = retweet_by.map(|rt_user| format!("retweet-{}", rt_user.username));
      let mut classes = vec!["timeline-item", &author_class];
      if let Some(ref rc) = retweet_class {
         classes.push(rc);
      }
      if !extra_class.is_empty() {
         classes.push(extra_class);
      }
      if thread_ctx == ThreadContext::End {
         classes.push("thread-last");
      }
      let class = classes.join(" ");

      let tweet_link = format!(
         "/{}/status/{}#m",
         display_tweet.user.username, display_tweet.id
      );

      let content_class = if prefs.is_some_and(|pref| pref.bidi_support) {
         "tweet-content media-body tweet-bidi"
      } else {
         "tweet-content media-body"
      };

      // Pre-render tweet text + location as HTML string for verbatim output
      let text_html = render_tweet_text_html(
         &display_tweet.text,
         &display_tweet.entities,
         &display_tweet.location,
         config,
      );

      html! {
          div class=(class) data-username=(display_tweet.user.username) {
              // Tweet link overlay (for clicking anywhere on tweet)
              @if !is_main {
                  a class="tweet-link" href=(&tweet_link) {}
              }

              div class="tweet-body" {
                  // Wrapper div around pinned/retweet + header
                  div {
                      // Pinned badge
                      @if pinned {
                          div class="pinned" {
                              span { (icon("pin", "Pinned Tweet", "", "", "")) }
                          }
                      }

                      // Retweet header
                      @if let Some(rt_user) = retweet_by {
                          div class="retweet-header" {
                              span { (icon("retweet", &format!("{} retweeted", rt_user.fullname), "", "", "")) }
                          }
                      }

                      // Tweet header with avatar, name row, and date
                      div class="tweet-header" {
                          a class="tweet-avatar" href=(format!("/{}", display_tweet.user.username)) {
                              (render_user_avatar(&display_tweet.user, config, prefs))
                          }

                          div class="tweet-name-row" {
                              div class="fullname-and-username" {
                                  (link_user(&display_tweet.user, "fullname"))
                                  (verified_icon(&display_tweet.user))
                                  (link_user(&display_tweet.user, "username"))
                              }

                              span class="tweet-date" {
                                  @if let Some(time) = display_tweet.time {
                                      a href=(&tweet_link) title=(formatters::format_tweet_time(time)) {
                                          (formatters::format_relative_time(time))
                                      }
                                  }
                              }
                          }
                      }
                  }

                  // Reply indicator: only shown for the first tweet (index 0) in a
                  // thread group, not for main tweets, not in thread context (conversation
                  // page), and not for self-replies.
                  @if !is_main && index == 0 && thread_ctx == ThreadContext::None && !display_tweet.reply.is_empty() && !is_reply_to_self(display_tweet, pinned) {
                      (render_reply(display_tweet))
                  }

                  // Tweet content (verbatim text + location, no <p> wrapper)
                  div class=(content_class) dir="auto" {
                      (PreEscaped(&text_html))
                  }

                  // Content disclosure labels
                  @if display_tweet.paid_promotion || display_tweet.ai_generated {
                      @let paid_label = format!("{} Paid partnership", config.config.paid_emoji);
                      @let ai_label = format!("{} Made with AI", config.config.ai_emoji);
                      @if is_main {
                          @if display_tweet.paid_promotion {
                              div class="disclosure-label" { (paid_label) }
                          }
                          @if display_tweet.ai_generated {
                              div class="disclosure-label" { (ai_label) }
                          }
                      } @else {
                          @let labels: Vec<&str> = [
                              display_tweet.paid_promotion.then_some(paid_label.as_str()),
                              display_tweet.ai_generated.then_some(ai_label.as_str()),
                          ].into_iter().flatten().collect();
                          div class="disclosure-label" { (labels.join(" · ")) }
                      }
                  }

                  // Attribution (when another user owns the content) — after tweet-content
                  @if let Some(ref attr_user) = display_tweet.attribution {
                      a class="attribution" href=(format!("/{}", attr_user.username)) {
                          (render_mini_avatar(attr_user, config, prefs))
                          strong { (attr_user.fullname) }
                          (verified_icon(attr_user))
                      }
                  }

                  // Card preview (before media)
                  @if let Some(ref card) = display_tweet.card {
                      (render_card(card, config, prefs))
                  }

                  // Media (photos)
                  @if !display_tweet.photos.is_empty() {
                      (render_photos(&display_tweet.photos, config))
                  }

                  // Video
                  @if let Some(ref video) = display_tweet.video {
                      (render_video(video, config, prefs))
                  }

                  // GIF
                  @if let Some(ref gif) = display_tweet.gif {
                      @let autoplay_gifs = prefs.is_none_or(|pref| pref.autoplay_gifs);
                      @let poster = get_small_pic(&gif.thumb, config);
                      @let gif_src = formatters::get_pic_url(&gif.url, config.config.base64_media);
                      div class="attachments media-gif" {
                          div class="gallery-gif" style="max-height: unset" {
                              div class="attachment" {
                                  video class="gif" poster=(poster) autoplay[autoplay_gifs] controls="" muted="" loop="" {
                                      source src=(gif_src) type="video/mp4";
                                  }
                              }
                          }
                      }
                  }

                  // Poll
                  @if let Some(ref poll) = display_tweet.poll {
                      (render_poll(poll, config))
                  }

                  // Quote tweet
                  @if let Some(ref quote) = display_tweet.quote {
                      (render_quote(quote, config, prefs))
                  }

                  // Community note
                  (render_community_note(display_tweet.note.as_ref(), prefs.is_some_and(|pref| pref.hide_community_notes)))

                  // Published time for main tweet
                  @if is_main {
                      @let has_edits = display_tweet.history.len() > 1;
                      @let is_latest = has_edits && display_tweet.id == display_tweet.history.iter().copied().max().unwrap_or(0);
                      @if let Some(time) = display_tweet.time {
                          p class="tweet-published" {
                              @if has_edits && is_latest {
                                  a href=(format!("/{}/status/{}/history", display_tweet.user.username, display_tweet.id)) {
                                      "Last edited " (formatters::format_tweet_time(time))
                                  }
                              } @else {
                                  (formatters::format_tweet_time(time))
                              }
                          }
                      }
                      @if has_edits && !is_latest {
                          @let latest_id = display_tweet.history.iter().copied().max().unwrap_or(0);
                          div class="latest-post-version" {
                              "There's a new version of this post. "
                              a href=(format!("/{}/status/{latest_id}#m", display_tweet.user.username)) {
                                  "See the latest post"
                              }
                          }
                      }
                  }

                  // Media tags
                  @if !display_tweet.media_tags.is_empty() {
                      div class="media-tag-block" {
                          (icon("user", "", "", "", ""))
                          @for (i, user) in display_tweet.media_tags.iter().enumerate() {
                              a class="media-tag" href=(format!("/{}", user.username)) title=(user.username) {
                                  (user.fullname)
                              }
                              @if i < display_tweet.media_tags.len() - 1 {
                                  ", "
                              }
                          }
                      }
                  }

                  // Stats
                  @if !prefs.is_some_and(|pref| pref.hide_tweet_stats) {
                      (render_stats(&display_tweet.stats, &display_tweet.user.username, display_tweet.id, self.sort_toggle))
                  } @else if let Some(toggle) = self.sort_toggle {
                      div class="tweet-stats" { (toggle) }
                  }
              }
          }
      }
   }
}

/// Render photos in attachments gallery with row distribution.
///
/// Photos are grouped into rows:
/// - 1-2 photos: single row
/// - 3+ photos: distributed into 2 rows (e.g. 3 → [2, 1], 4 → [2, 2])
fn render_photos(photos: &[Photo], config: &Config) -> Markup {
   let groups: Vec<&[Photo]> = if photos.len() < 3 {
      vec![photos]
   } else {
      let mid = photos.len() / 2 + photos.len() % 2;
      let (top, bottom) = photos.split_at(mid);
      vec![top, bottom]
   };

   html! {
       div class="attachments" {
           @for (i, group) in groups.iter().enumerate() {
               @let margin = if i > 0 { "margin-top: .25em" } else { "" };
               div class="gallery-row" style=(margin) {
                   @for photo in *group {
                       @let named = photo.url.contains("name=");
                       @let small = if named {
                           formatters::get_pic_url(&photo.url, config.config.base64_media)
                       } else {
                           get_small_pic(&photo.url, config)
                       };
                       @let orig_url = formatters::get_orig_pic_url(&photo.url, config.config.base64_media);
                       div class="attachment image" {
                           a class="still-image" href=(orig_url) target="_blank" {
                               img src=(small) alt=(photo.alt_text) loading="lazy";
                           }
                           @if !photo.alt_text.is_empty() {
                               p class="alt-text" { "ALT  " (photo.alt_text) }
                           }
                       }
                   }
               }
           }
       }
   }
}

/// Check if tweet is replying only to self (unless pinned).
fn is_reply_to_self(tweet: &Tweet, pinned: bool) -> bool {
   !pinned
      && tweet.reply.len() == 1
      && tweet.reply[0].to_lowercase() == tweet.user.username.to_lowercase()
}

/// Render reply indicator showing who the tweet is replying to.
/// Uses class "replying-to".
fn render_reply(tweet: &Tweet) -> Markup {
   html! {
       div class="replying-to" {
           "Replying to "
           @for (i, username) in tweet.reply.iter().enumerate() {
               @if i > 0 {
                   " "
               }
               a href=(format!("/{username}")) { "@" (username) }
           }
       }
   }
}

/// Render a mini avatar image for quote tweets.
fn render_mini_avatar(user: &User, config: &Config, prefs: Option<&Prefs>) -> Markup {
   let avatar_class = if prefs.is_some_and(|pref| pref.square_avatars) {
      "avatar mini"
   } else {
      "avatar round mini"
   };
   let pic = formatters::get_user_pic(&user.user_pic, "_mini");
   let pic_url = formatters::get_pic_url(&pic, config.config.base64_media);
   html! {
       img src=(pic_url) class=(avatar_class) alt="" loading="lazy";
   }
}

/// Render a quote tweet with dedicated structure.
fn render_quote(quote: &Tweet, config: &Config, prefs: Option<&Prefs>) -> Markup {
   if !quote.available {
      let has_id = quote.id != 0;
      let user = if quote.user.username.is_empty() {
         "i"
      } else {
         &quote.user.username
      };
      return html! {
          div class="quote unavailable" {
              div class="unavailable-quote" {
                  @if !quote.tombstone.is_empty() {
                      (quote.tombstone)
                  } @else if !quote.text.is_empty() {
                      (quote.text)
                  } @else {
                      "This quoted post is unavailable."
                  }
                  @if has_id {
                      div class="unavailable-actions" {
                          a href=(format!("/{user}/status/{}", quote.id)) { "Try viewing anyway" }
                          " · "
                          a href=(format!("https://x.com/{user}/status/{}", quote.id)) { "View on X" }
                      }
                  }
              }
          }
      };
   }

   let quote_link = format!("/{}/status/{}", quote.user.username, quote.id);
   let text_html = render_tweet_text_html(&quote.text, &quote.entities, "", config);

   html! {
       div class="quote quote-big" {
           a class="quote-link" href=(&quote_link) {}

           div class="tweet-name-row" {
               div class="fullname-and-username" {
                   (render_mini_avatar(&quote.user, config, prefs))
                   (link_user(&quote.user, "fullname"))
                   (verified_icon(&quote.user))
                   (link_user(&quote.user, "username"))
               }

               span class="tweet-date" {
                   @if let Some(time) = quote.time {
                       a href=(&quote_link) title=(formatters::format_tweet_time(time)) {
                           (formatters::format_relative_time(time))
                       }
                   }
               }
           }

           @if !quote.reply.is_empty() {
               (render_reply(quote))
           }

           @if !quote.text.is_empty() {
               div class="quote-text" dir="auto" {
                   (PreEscaped(&text_html))
               }
           }

           // Card preview in quote
           @if let Some(ref card) = quote.card {
               (render_card(card, config, prefs))
           }

           // Media
           @if !quote.photos.is_empty() {
               (render_photos(&quote.photos, config))
           }
           @if let Some(ref video) = quote.video {
               (render_video(video, config, prefs))
           }
           @if let Some(ref gif) = quote.gif {
               @let gif_url = formatters::get_vid_url(&gif.url, &config.config.hmac_key, config.config.base64_media);
               div class="attachments media-gif" {
                   div class="gallery-gif" style="max-height: unset" {
                       div class="attachment" {
                           video autoplay="" loop="" muted="" playsinline="" {
                               source src=(gif_url) type="video/mp4";
                           }
                       }
                   }
               }
           }

           // Poll
           @if let Some(ref poll) = quote.poll {
               (render_poll(poll, config))
           }

           // Community note for quoted tweet
           (render_community_note(quote.note.as_ref(), prefs.is_some_and(|pref| pref.hide_community_notes)))

           @if quote.has_thread {
               a class="show-thread" href=(&quote_link) {
                   "Show this thread"
               }
           }

           // "There's a new version of this post" for edited quotes
           @if quote.history.len() > 1 {
               @let max_id = quote.history.iter().copied().max().unwrap_or(0);
               @if quote.id != max_id {
                   div class="quote-latest" {
                       "There's a new version of this post"
                   }
               }
           }
       }
   }
}

/// Render poll with options and vote percentages.
#[expect(
   clippy::cast_precision_loss,
   clippy::cast_sign_loss,
   clippy::cast_possible_truncation,
   reason = "poll vote counts fit in f64, leader index is always 0-3"
)]
fn render_poll(poll: &Poll, config: &Config) -> Markup {
   let total_votes = poll.votes.max(1); // Avoid division by zero
   let leader_idx = poll.leader as usize;
   let percentages = poll
      .values
      .iter()
      .map(|&val| {
         if val > 0 {
            val as f64 / total_votes as f64 * 100.0
         } else {
            0.0
         }
      })
      .collect::<Vec<_>>();
   let has_images = !poll.choice_images.is_empty();

   html! {
       div class="poll" {
           @if has_images {
               div class="poll-choice-images" {
                   @for img in &poll.choice_images {
                       (gen_img(img, "poll-choice-img", config))
                   }
               }
           }
           @for (idx, option) in poll.options.iter().enumerate() {
               @let perc = percentages.get(idx).copied().unwrap_or(0.0);
               @let perc_str = format!("{perc:.0}%");
               @let is_leader = idx == leader_idx;
               @let class = if is_leader { "poll-meter leader" } else { "poll-meter" };

               div class=(class) {
                   span class="poll-choice-bar" style=(format!("width: {perc_str}")) {}
                   span class="poll-choice-value" { (&perc_str) }
                   span class="poll-choice-option" { (option) }
               }
           }

           span class="poll-info" {
               (format!("{} votes \u{2022} {}", formatters::format_with_commas(poll.votes), poll.status_text()))
           }
       }
   }
}

/// Render card image container.
fn render_card_image(card: &Card, config: &Config) -> Markup {
   html! {
       div class="card-image-container" {
           div class="card-image" {
               (gen_img(&card.image, "", config))
               @if matches!(card.kind, CardKind::Player) {
                   div class="card-overlay" {
                       div class="overlay-circle" {
                           span class="overlay-triangle" {}
                       }
                   }
               }
           }
       }
   }
}

/// Render card content.
fn render_card_content(card: &Card) -> Markup {
   html! {
       div class="card-content" {
           h2 class="card-title" { (card.title) }
           @if !card.text.is_empty() {
               p class="card-description" { (card.text) }
           }
           // Format destination: "List · 42 Members" for list/community, else raw dest
           @if card.member_count > 0 && !card.dest.is_empty() {
               span class="card-destination" {
                   (format!("{} · {} Members", card.dest, card.member_count))
               }
           } @else if !card.dest.is_empty() {
               span class="card-destination" { (card.dest) }
           }
       }
   }
}

/// Render card preview for links.
///
/// Small cards: App, Player, Summary, `StoreLink`.
/// Large cards: everything else (`SummaryLarge`, `PromoWebsite`, etc.).
fn render_card(card: &Card, config: &Config, prefs: Option<&Prefs>) -> Markup {
   // Skip hidden or unknown cards
   if matches!(card.kind, CardKind::Hidden | CardKind::Unknown) {
      return html! {};
   }

   let is_small = matches!(
      card.kind,
      CardKind::App | CardKind::Player | CardKind::Summary | CardKind::StoreLink
   );
   let large = if is_small { "" } else { " large" };
   let class = format!("card{large}");

   html! {
       div class=(class) {
           @if let Some(ref video) = card.video {
               div class="card-container" {
                   (render_video(video, config, prefs))
                   a class="card-content-container" href=(card.url) {
                       (render_card_content(card))
                   }
               }
           } @else {
               a class="card-container" href=(card.url) {
                   @if !card.image.is_empty() {
                       (render_card_image(card, config))
                   }
                   div class="card-content-container" {
                       (render_card_content(card))
                   }
               }
           }
       }
   }
}

/// Render user avatar image with proper class from prefs.
fn render_user_avatar(user: &User, config: &Config, prefs: Option<&Prefs>) -> Markup {
   let avatar_url = formatters::get_pic_url(&user.user_pic, config.config.base64_media);
   let avatar_class = get_avatar_class(prefs);
   html! {
       img src=(avatar_url) class=(avatar_class) alt="" loading="lazy";
   }
}

/// Render tweet text and location as a single HTML string (for verbatim
/// output). This avoids wrapping in an extra `<p>` tag — the original version
/// uses `verbatim` directly inside the content div.
fn render_tweet_text_html(
   text: &str,
   entities: &[Entity],
   location: &str,
   config: &Config,
) -> String {
   // Use entity expansion if entities are available
   let processed = if entities.is_empty() {
      expand_with_regex(text)
   } else {
      expand_entities(text, entities)
   };

   // Apply URL domain replacements
   let mut result = formatters::replace_urls(&processed, config);

   // Append location if present
   if !location.is_empty() {
      result.push_str(&render_location_html(location));
   }

   result
}

/// Render video player.
///
/// Structure: `attachments card` > `gallery-video [card-container]` >
///   `attachment video-container` > video/img + overlay.
fn render_video(video: &Video, config: &Config, prefs: Option<&Prefs>) -> Markup {
   let has_card_content = !video.description.is_empty() || !video.title.is_empty();
   let container = if has_card_content {
      " card-container"
   } else {
      ""
   };

   let thumb = if video.thumb.is_empty() {
      String::new()
   } else {
      get_small_pic(&video.thumb, config)
   };

   let duration = formatters::format_duration(video.duration_ms);

   let playback_enabled = prefs.is_none_or(|pref| pref.mp4_playback);

   let mute_videos = prefs.is_some_and(|pref| pref.mute_videos);

   html! {
       div class="attachments card" {
           div class=(format!("gallery-video{container}")) {
               div class="attachment video-container" {
                   @if !video.available {
                       img src=(thumb) loading="lazy";
                       div class="video-overlay" {
                           @if video.reason == "dmcaed" {
                               p { "This media has been disabled in response to a report by the copyright owner" }
                           } @else {
                               p { "This media is unavailable" }
                           }
                       }
                   } @else if !playback_enabled {
                       // Playback disabled in preferences
                       img src=(thumb) loading="lazy";
                       div class="video-overlay" {
                           (button_referer("/enablemp4", "Enable mp4 playback", "", "", ""))
                       }
                   } @else if let Some(mp4_url) = video.best_mp4_url() {
                       // MP4 playback: native video element with controls
                       @let video_src = formatters::get_vid_url(mp4_url, &config.config.hmac_key, config.config.base64_media);
                       video poster=(thumb) controls="" muted=[mute_videos.then_some("")] {
                           source src=(video_src) type="video/mp4";
                       }
                   } @else {
                       // No MP4 URL available - show thumbnail with duration
                       img src=(thumb) loading="lazy";
                       div class="video-overlay" {
                           div class="overlay-duration" { (duration) }
                       }
                   }
               }
               @if has_card_content {
                   div class="card-content" {
                       h2 class="card-title" { (video.title) }
                       @if !video.description.is_empty() {
                           p class="card-description" { (video.description) }
                       }
                   }
               }
           }
       }
   }
}

/// Render tweet stats using `icon()` helper from renderutils.
/// Retweet count links to the retweeters page; quote count links to search.
/// Optional `extra` markup is appended after the stat icons (used for the
/// reply-sort toggle on the main tweet).
fn render_stats(stats: &TweetStats, username: &str, id: i64, extra: Option<&Markup>) -> Markup {
   let fmt = |n: i64| {
      if n > 0 {
         formatters::format_with_commas(n)
      } else {
         String::new()
      }
   };
   html! {
       div class="tweet-stats" {
           span class="tweet-stat" { (icon("comment", &fmt(stats.replies), "", "", "")) }
           a class="tweet-stat" href=(format!("/{username}/status/{id}/retweets")) title="Retweets" {
               (icon("retweet", &fmt(stats.retweets), "", "", ""))
           }
           @if stats.quotes > 0 {
               a class="tweet-stat" href=(format!("/{username}/status/{id}/quotes")) title="Quotes" {
                   (icon("quote", &fmt(stats.quotes), "", "", ""))
               }
           }
           span class="tweet-stat" { (icon("heart", &fmt(stats.likes), "", "", "")) }
           span class="tweet-stat" { (icon("views", &fmt(stats.views), "", "", "")) }
           @if let Some(extra) = extra {
               (extra)
           }
       }
   }
}

/// Render tweet location as HTML string (for concatenation with tweet text).
fn render_location_html(location: &str) -> String {
   use crate::utils::html_escape;
   let (place, url) = parse_location(location);
   let place = html_escape(&place);
   if url.is_empty() {
      format!(r#"<span class="tweet-geo"> – at {place}</span>"#)
   } else {
      let url = html_escape(&url);
      format!(r#"<span class="tweet-geo"> – at <a href="{url}">{place}</a></span>"#)
   }
}

/// Render reply chains with thread context and optional "Load more" link.
/// Shared between AJAX scroll and full-page rendering in status.rs.
pub fn render_reply_chains(
   chains: &[Chain],
   bottom_cursor: &str,
   username: &str,
   id: &str,
   config: &Config,
   prefs: &Prefs,
) -> Markup {
   html! {
       @for chain in chains {
           @if !chain.content.is_empty() {
               div class="reply thread thread-line" {
                   @let chain_len = chain.content.len();
                   @let chain_has_more = chain.has_more;
                   @for (idx, tweet) in chain.content.iter().enumerate() {
                       @let is_last = idx == chain_len - 1 && !chain_has_more;
                       @let ctx = thread_context(idx, chain_len, is_last);
                       (TweetRenderer::new(tweet, config, false).prefs(prefs).thread_ctx(ctx).render())
                   }
                   @if chain_has_more {
                       @if let Some(last_in_chain) = chain.content.last() {
                           div class="timeline-item more-replies" {
                               @if last_in_chain.available {
                                   a class="more-replies-text" href=(format!("/{}/status/{}#m", last_in_chain.user.username, last_in_chain.id)) {
                                       "more replies"
                                   }
                               } @else {
                                   a class="more-replies-text" { "more replies" }
                               }
                           }
                       }
                   }
               }
           }
       }
       @if !bottom_cursor.is_empty() {
           div class="show-more" {
               a href=(format!("{}#r", formatters::cursor_url(&format!("/{username}/status/{id}"), bottom_cursor))) {
                   "Load more replies"
               }
           }
       }
   }
}
