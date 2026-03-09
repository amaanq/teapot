use maud::{
   Markup,
   html,
};

use super::tweet::TweetRenderer;
use crate::{
   config::Config,
   types::{
      List,
      Prefs,
      TimelineKind,
      Tweet,
      Tweets,
   },
   utils::formatters,
   views::renderutils::{
      gen_img,
      icon,
   },
};

/// Render a timeline of tweets with optional "Load more" link.
#[expect(
   clippy::module_name_repetitions,
   reason = "render_timeline is the canonical name"
)]
pub fn render_timeline(
   groups: &[Tweets],
   config: &Config,
   cursor: Option<&str>,
   base_url: Option<&str>,
) -> Markup {
   render_timeline_full(groups, config, cursor, base_url, None, None, None)
}

/// Render a timeline with prefs support (for bidi).
pub fn render_timeline_with_prefs(
   groups: &[Tweets],
   config: &Config,
   cursor: Option<&str>,
   base_url: Option<&str>,
   prefs: &Prefs,
   newer_url: Option<&str>,
) -> Markup {
   render_timeline_full(
      groups,
      config,
      cursor,
      base_url,
      None,
      Some(prefs),
      newer_url,
   )
}

/// Render a timeline with pinned tweet and optional prefs support.
pub fn render_timeline_with_pinned_and_prefs(
   groups: &[Tweets],
   config: &Config,
   cursor: Option<&str>,
   base_url: Option<&str>,
   pinned: Option<&Tweet>,
   prefs: Option<&Prefs>,
   newer_url: Option<&str>,
) -> Markup {
   render_timeline_full(groups, config, cursor, base_url, pinned, prefs, newer_url)
}

/// Render "scroll to top" button.
pub fn render_to_top_with_focus(focus: &str) -> Markup {
   html! {
       div class="top-ref" {
           (icon("down", "", "", "", focus))
       }
   }
}

fn render_to_top() -> Markup {
   render_to_top_with_focus("#")
}

/// Render "No more items" footer.
fn render_no_more() -> Markup {
   html! {
       div class="timeline-footer" {
           h2 class="timeline-end" { "No more items" }
       }
   }
}

/// Render "No items found" header.
fn render_none_found() -> Markup {
   html! {
       div class="timeline-header" {
           h2 class="timeline-none" { "No items found" }
       }
   }
}

use super::renderutils::tweet_link;

/// Render a thread group wrapped in thread-line.
fn render_thread(thread: &[&Tweet], config: &Config, prefs: Option<&Prefs>) -> Markup {
   // Sort by ID for correct order
   let mut sorted: Vec<&Tweet> = thread.to_vec();
   sorted.sort_by_key(|tweet| tweet.id);

   html! {
       div class="thread-line" {
           @for (idx, tweet) in sorted.iter().enumerate() {
               // Detect gap: if this tweet's reply_id doesn't match the previous tweet's id
               @if idx > 0 && tweet.reply_id != sorted[idx - 1].id {
                   div class="timeline-item thread more-replies-thread" {
                       div class="more-replies" {
                           a class="more-replies-text" href=(tweet_link(tweet)) {
                               "more replies"
                           }
                       }
                   }
               }
               // Render the tweet with thread class ("thread" + optional "with-header" for pinned/retweet)
               @let is_last = idx == sorted.len() - 1;
               @let show_thread = is_last && sorted[0].id != tweet.thread_id;
               @let has_header = tweet.pinned || tweet.retweet.is_some();
               @let thread_class = match (has_header, is_last) {
                   (true, true) => "with-header thread thread-last",
                   (true, false) => "with-header thread",
                   (false, true) => "thread thread-last",
                   (false, false) => "thread",
               };
               (TweetRenderer::new(tweet, config, false).maybe_prefs(prefs).extra_class(thread_class).index(idx).render())
               @if show_thread && tweet.has_thread {
                   div class="show-thread" {
                       a href=(tweet_link(tweet)) { "Show this thread" }
                   }
               }
           }
       }
   }
}

/// Full timeline rendering with all options.
/// `groups` preserves conversation structure from the API: each inner
/// Vec<Tweet> is a conversation thread (parent -> reply chain).
fn render_timeline_full(
   groups: &[Tweets],
   config: &Config,
   cursor: Option<&str>,
   base_url: Option<&str>,
   pinned: Option<&Tweet>,
   prefs: Option<&Prefs>,
   newer_url: Option<&str>,
) -> Markup {
   let load_more_url = match (cursor, base_url) {
      (Some(cur), Some(base)) => Some(formatters::cursor_url(base, cur)),
      (Some(cur), None) => Some(formatters::cursor_url("", cur)),
      _ => None,
   };

   // Get pinned tweet ID to avoid duplicates
   let pinned_id = pinned.map(|tweet| tweet.id);
   let has_tweets = groups.iter().any(|group| !group.is_empty()) || pinned.is_some();

   html! {
       div class="timeline" {
           // "Load newest" link when viewing paginated results
           @if let Some(url) = newer_url {
               div class="timeline-item show-more" {
                   a href=(url) { "Load newest" }
               }
           }

           @if !has_tweets {
               (render_none_found())
           } @else {
               // Render pinned tweet first
               @if let Some(pinned_tweet) = pinned {
                   @if !prefs.is_some_and(|pref| pref.hide_pins) {
                       (TweetRenderer::new(pinned_tweet, config, false).pinned(true).maybe_prefs(prefs).render())
                   }
               }

               // Render groups: each group is a conversation thread from the API
               @for group in groups {
                   // Filter out pinned duplicates
                   @let filtered = group.iter().filter(|tweet| Some(tweet.id) != pinned_id).collect::<Vec<_>>();
                   @if filtered.len() > 1 {
                       (render_thread(&filtered, config, prefs))
                   } @else if let Some(tweet) = filtered.first() {
                       (TweetRenderer::new(tweet, config, false).maybe_prefs(prefs).render())
                       // "Show this thread" for standalone tweets that have threads
                       @if tweet.has_thread {
                           div class="show-thread" {
                               a href=(tweet_link(tweet)) { "Show this thread" }
                           }
                       }
                   }
               }

               // Pagination
               @if let Some(ref url) = load_more_url {
                   div class="show-more" {
                       a href=(url) { "Load more" }
                   }
               } @else if has_tweets {
                   (render_no_more())
               }

               // Scroll to top
               @if has_tweets {
                   (render_to_top())
               }
           }
       }
   }
}

/// Render timeline with tabs (tweets, replies, media).
pub fn render_timeline_tabs(active_tab: TimelineKind, username: &str) -> Markup {
   html! {
       ul class="tab" {
           li class=(if active_tab == TimelineKind::Tweets { "tab-item active" } else { "tab-item" }) {
               a href=(format!("/{username}")) { "Tweets" }
           }
           li class=(if active_tab == TimelineKind::Replies { "tab-item active wide" } else { "tab-item wide" }) {
               a href=(format!("/{username}/with_replies")) { "Tweets & Replies" }
           }
           li class=(if active_tab == TimelineKind::Media { "tab-item active" } else { "tab-item" }) {
               a href=(format!("/{username}/media")) { "Media" }
           }
           li class=(if active_tab == TimelineKind::Search { "tab-item active" } else { "tab-item" }) {
               a href=(format!("/{username}/search")) { "Search" }
           }
       }
   }
}

/// Convert tab string to `TimelineKind`.
pub fn tab_to_kind(tab: &str) -> TimelineKind {
   match tab {
      "replies" | "with_replies" => TimelineKind::Replies,
      "media" => TimelineKind::Media,
      "search" => TimelineKind::Search,
      _ => TimelineKind::Tweets,
   }
}

/// Render list header (banner + name + tabs).
/// Moved here from routes/list.rs since it's presentation logic.
pub fn render_list_header(list: &List, active_tab: &str, config: &Config) -> Markup {
   let path = format!("/i/lists/{}", list.id);
   let tweets_class = if active_tab == "tweets" {
      "tab-item active"
   } else {
      "tab-item"
   };
   let members_class = if active_tab == "members" {
      "tab-item active"
   } else {
      "tab-item"
   };

   html! {
       @if !list.banner.is_empty() {
           div class="timeline-banner" {
               a href=(formatters::get_pic_url(&list.banner, config.config.base64_media)) target="_blank" {
                   (gen_img(&list.banner, "", config))
               }
           }
       }

       div class="timeline-header" {
           "\"" (list.name) "\" by @" (list.username)

           div class="timeline-description" {
               (list.description)
           }
       }

       ul class="tab" {
           li class=(tweets_class) {
               a href=(&path) { "Tweets" }
           }
           li class=(members_class) {
               a href=(format!("{path}/members")) { "Members" }
           }
       }
   }
}
