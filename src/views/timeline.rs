use topcoat::{
   Result,
   context::Cx,
   view::{
      View,
      class,
      component,
      view,
   },
};

use crate::{
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
   views::{
      renderutils::{
         config,
         icon,
         proxied_image,
         tweet_link,
      },
      tweet::tweet_item,
   },
};

/// Render "scroll to top" button.
#[component]
pub async fn to_top(#[default("#")] focus: &str) -> Result<impl View> {
   Ok(view! {
      <div class="top-ref">icon(name: "down", href: focus)</div>
   })
}

/// Render a thread group wrapped in thread-line.
#[component]
async fn thread_group<'a>(thread: &'a [&'a Tweet], prefs: &'a Prefs) -> Result<impl View> {
   let mut sorted = thread.to_vec();
   sorted.sort_by_key(|tweet| tweet.id);
   let last = sorted.len() - 1;
   let root_id = sorted[0].id;
   // Detect a gap when reply_id does not match the previous tweet's ID
   let gaps = sorted
      .iter()
      .enumerate()
      .map(|(position, tweet)| position > 0 && tweet.reply_id != sorted[position - 1].id)
      .collect::<Vec<_>>();

   Ok(view! {
      <div class="thread-line">
         #[key(tweet.id)]
         for (position, tweet) in sorted.iter().copied().enumerate() {
            if gaps[position] {
               <div class="timeline-item thread more-replies-thread">
                  <div class="more-replies">
                     <a class="more-replies-text" href=(tweet_link(tweet))>"more replies"</a>
                  </div>
               </div>
            }
            let has_header = tweet.pinned || tweet.retweet.is_some();
            let place = if position == last {
               "thread-last"
            } else if position == 0 {
               "thread-first"
            } else {
               "thread-middle"
            };
            let classes = format!("{}thread {place}", if has_header { "with-header " } else { "" });
            tweet_item(
               tweet: tweet,
               prefs: prefs,
               extra_class: &classes,
               index: position,
            )
            if position == last && root_id != tweet.thread_id && tweet.has_thread {
               <div class="show-thread">
                  <a href=(tweet_link(tweet))>"Show this thread"</a>
               </div>
            }
         }
      </div>
   })
}

/// `groups` preserves conversation structure from the API. Each inner
/// Vec<Tweet> is a conversation thread (parent -> reply chain).
#[component]
pub async fn timeline<'a>(
   groups: &'a [Tweets],
   prefs: &'a Prefs,
   #[default] cursor: Option<&'a str>,
   #[default] base_url: &'a str,
   #[default] pinned: Option<&'a Tweet>,
   #[default] newer_url: Option<&'a str>,
) -> Result<impl View> {
   let load_more_url = cursor.map(|cursor| formatters::cursor_url(base_url, cursor));
   // Get pinned tweet ID to avoid duplicates
   let pinned_id = pinned.map(|tweet| tweet.id);
   let has_tweets = groups.iter().any(|group| !group.is_empty()) || pinned.is_some();
   let groups = groups
      .iter()
      .map(|group| {
         group
            .iter()
            .filter(|tweet| Some(tweet.id) != pinned_id)
            .collect::<Vec<_>>()
      })
      .collect::<Vec<_>>();

   Ok(view! {
      <div class="timeline">
         if let Some(url) = newer_url {
            <div class="timeline-item show-more"><a href=(url)>"Load newest"</a></div>
         }
         if !has_tweets {
            <div class="timeline-header"><h2 class="timeline-none">"No items found"</h2></div>
         } else {
            if let Some(tweet) = pinned.filter(|_| !prefs.hide_pins) {
               tweet_item(tweet: tweet, prefs: prefs, pinned: true)
            }
            #[key(group.first().map_or(0, |tweet| tweet.id))]
            for group in &groups {
               if group.len() > 1 {
                  thread_group(thread: group, prefs: prefs)
               } else if let Some(tweet) = group.first().copied() {
                  tweet_item(tweet: tweet, prefs: prefs)
                  if tweet.has_thread {
                     <div class="show-thread"><a href=(tweet_link(tweet))>"Show this thread"</a></div>
                  }
               }
            }
            if let Some(url) = load_more_url {
               <div class="show-more"><a href=(url)>"Load more"</a></div>
            } else {
               <div class="timeline-footer"><h2 class="timeline-end">"No more items"</h2></div>
            }
            to_top()
         }
      </div>
   })
}

const PROFILE_TABS: [(TimelineKind, &str, &str); 4] = [
   (TimelineKind::Tweets, "", "Tweets"),
   (TimelineKind::Replies, "/with_replies", "Tweets & Replies"),
   (TimelineKind::Media, "/media", "Media"),
   (TimelineKind::Search, "/search", "Search"),
];

/// Render timeline with tabs (tweets, replies, media).
#[component]
pub async fn timeline_tabs(active: TimelineKind, username: &str) -> Result<impl View> {
   Ok(view! {
      <ul class="tab">
         for (kind, suffix, label) in PROFILE_TABS {
            <li class=(class!("tab-item", "active" if kind == active, "wide" if kind == TimelineKind::Replies))>
               <a href=(format!("/{username}{suffix}"))>(label)</a>
            </li>
         }
      </ul>
   })
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
#[component]
pub async fn list_header(cx: &Cx, list: &List, active: ListTab) -> Result<impl View> {
   let path = format!("/i/lists/{}", list.id);
   Ok(view! {
      if !list.banner.is_empty() {
         <div class="timeline-banner">
            <a href=(formatters::get_pic_url(&list.banner, config(cx).config.base64_media)) target="_blank">
               proxied_image(url: &list.banner)
            </a>
         </div>
      }
      <div class="timeline-header">
         "\"" (&list.name) "\" by @" (&list.username)
         <div class="timeline-description">(&list.description)</div>
      </div>
      <ul class="tab">
         <li class=(class!("tab-item", "active" if active == ListTab::Tweets))>
            <a href=(&path)>"Tweets"</a>
         </li>
         <li class=(class!("tab-item", "active" if active == ListTab::Members))>
            <a href=(format!("{path}/members"))>"Members"</a>
         </li>
      </ul>
   })
}
