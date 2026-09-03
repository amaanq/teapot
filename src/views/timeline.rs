use maud::{
   Markup,
   html,
};

use super::tweet::TweetRenderer;
use crate::{
   config::Config,
   types::{
      prefs::Prefs,
      timeline::{
         List,
         TimelineKind,
         Tweets,
      },
      tweet::Tweet,
   },
   utils::formatters,
   views::renderutils::{
      gen_img,
      icon,
   },
};

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
fn render_thread(thread: &[&Tweet], config: &Config, prefs: &Prefs) -> Markup {
   let mut sorted = thread.to_vec();
   sorted.sort_by_key(|tweet| tweet.id);

   html! {
       div class="thread-line" {
           @for (idx, tweet) in sorted.iter().enumerate() {
               // Detect a gap when reply_id does not match the previous tweet's ID
               @if idx > 0 && tweet.reply_id != sorted[idx - 1].id {
                   div class="timeline-item thread more-replies-thread" {
                       div class="more-replies" {
                           a class="more-replies-text" href=(tweet_link(tweet)) {
                               "more replies"
                           }
                       }
                   }
               }
               @let is_last = idx == sorted.len() - 1;
               @let show_thread = is_last && sorted[0].id != tweet.thread_id;
               @let has_header = tweet.pinned || tweet.retweet.is_some();
               @let thread_class = match (has_header, is_last) {
                   (true, true) => "with-header thread thread-last",
                   (true, false) if idx == 0 => "with-header thread thread-first",
                   (true, false) => "with-header thread thread-middle",
                   (false, true) => "thread thread-last",
                   (false, false) if idx == 0 => "thread thread-first",
                   (false, false) => "thread thread-middle",
               };
               (TweetRenderer::new(tweet, config, prefs, false).extra_class(thread_class).index(idx).render())
               @if show_thread && tweet.has_thread {
                   div class="show-thread" {
                       a href=(tweet_link(tweet)) { "Show this thread" }
                   }
               }
           }
       }
   }
}

/// `groups` preserves conversation structure from the API. Each inner
/// Vec<Tweet> is a conversation thread (parent -> reply chain).
#[expect(
   clippy::module_name_repetitions,
   reason = "render_timeline is the canonical name"
)]
pub fn render_timeline(
   groups: &[Tweets],
   config: &Config,
   cursor: Option<&str>,
   base_url: Option<&str>,
   pinned: Option<&Tweet>,
   prefs: &Prefs,
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
           @if let Some(url) = newer_url {
               div class="timeline-item show-more" {
                   a href=(url) { "Load newest" }
               }
           }

           @if !has_tweets {
               (render_none_found())
           } @else {
               @if let Some(pinned_tweet) = pinned {
                   @if !prefs.hide_pins {
                       (TweetRenderer::new(pinned_tweet, config, prefs, false).pinned(true).render())
                   }
               }

               @for group in groups {
                   @let filtered = group.iter().filter(|tweet| Some(tweet.id) != pinned_id).collect::<Vec<_>>();
                   @if filtered.len() > 1 {
                       (render_thread(&filtered, config, prefs))
                   } @else if let Some(tweet) = filtered.first() {
                       (TweetRenderer::new(tweet, config, prefs, false).render())
                       @if tweet.has_thread {
                           div class="show-thread" {
                               a href=(tweet_link(tweet)) { "Show this thread" }
                           }
                       }
                   }
               }

               @if let Some(ref url) = load_more_url {
                   div class="show-more" {
                       a href=(url) { "Load more" }
                   }
               } @else if has_tweets {
                   (render_no_more())
               }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListTab {
   Tweets,
   Members,
}

/// Render list header (banner + name + tabs).
/// Moved here from routes/list.rs since it's presentation logic.
pub fn render_list_header(list: &List, active_tab: ListTab, config: &Config) -> Markup {
   let path = format!("/i/lists/{}", list.id);
   let tweets_class = if active_tab == ListTab::Tweets {
      "tab-item active"
   } else {
      "tab-item"
   };
   let members_class = if active_tab == ListTab::Members {
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
