use maud::{
   Markup,
   html,
};

use super::tweet::TweetRenderer;
use crate::{
   config::Config,
   types::{
      Prefs,
      Tweets,
      User,
   },
   views::{
      renderutils::icon,
      user_list::render_user,
   },
};

/// Render the empty search page (home search bar).
pub fn render_search_page() -> Markup {
   html! {
       div class="panel-container" {
           div class="search-bar" {
               form method="get" action="/search" autocomplete="off" {
                   input type="hidden" name="f" value="tweets";
                   input type="text" name="q" autofocus="" placeholder="Search..." dir="auto";
                   button type="submit" { span class="icon-search" {} }
               }
           }
       }
   }
}

/// Render search results page with prefs support.
#[expect(
   clippy::too_many_arguments,
   reason = "search page has many orthogonal display options"
)]
pub fn render_search_results_with_prefs(
   query: &str,
   tweets: &Tweets,
   config: &Config,
   cursor: Option<&str>,
   prefs: Option<&Prefs>,
   filters: Option<&SearchFilters>,
   newer_url: Option<&str>,
   active_tab: &str,
) -> Markup {
   html! {
       div class="timeline-container" {
           div class="timeline-header" {
               (render_search_panel_with_action(query, filters, "/search"))
           }

           (render_search_tabs(query, active_tab))

           div class="timeline" {
               // "Load newest" link when viewing paginated results
               @if let Some(url) = newer_url {
                   div class="timeline-item show-more" {
                       a href=(url) { "Load newest" }
                   }
               }

               @if tweets.is_empty() {
                   div class="timeline-header" {
                       h2 class="timeline-none" { "No items found" }
                   }
               } @else {
                   @for tweet in tweets {
                       (TweetRenderer::new(tweet, config, false).maybe_prefs(prefs).render())
                   }

                   @if let Some(cur) = cursor {
                       div class="show-more" {
                           a href=(format!("/search?q={}&cursor={}", urlencoding::encode(query), cur)) {
                               "Load more"
                           }
                       }
                   } @else {
                       div class="timeline-footer" {
                           h2 class="timeline-end" { "No more items" }
                       }
                   }
               }
           }
       }
   }
}

/// Render user search results page.
pub fn render_user_search_results(
   query: &str,
   users: &[User],
   config: &Config,
   cursor: Option<&str>,
   newer_url: Option<&str>,
   prefs: Option<&Prefs>,
) -> Markup {
   html! {
       div class="timeline-container" {
           div class="timeline-header" {
               form method="get" action="/search" class="search-field" autocomplete="off" {
                   input type="hidden" name="f" value="users";
                   div class="pref-group pref-input pref-inline" {
                       input type="text" name="q" value=(query) placeholder="Enter username...";
                   }
                   button type="submit" { span class="icon-search" {} }
               }
           }

           (render_search_tabs(query, "users"))

           div class="timeline" {
               // "Load newest" link when viewing paginated results
               @if let Some(url) = newer_url {
                   div class="timeline-item show-more" {
                       a href=(url) { "Load newest" }
                   }
               }

               @if users.is_empty() {
                   div class="timeline-header" {
                       h2 class="timeline-none" { "No items found" }
                   }
               } @else {
                   @for user in users {
                       (render_user(user, config, prefs))
                   }

                   @if let Some(cur) = cursor {
                       div class="show-more" {
                           a href=(format!("/search?q={}&f=users&cursor={}", urlencoding::encode(query), cur)) {
                               "Load more"
                           }
                       }
                   } @else {
                       div class="timeline-footer" {
                           h2 class="timeline-end" { "No more items" }
                       }
                   }
               }
           }
       }
   }
}

/// Render search tabs (Top / Latest / Media / Users).
fn render_search_tabs(query: &str, active: &str) -> Markup {
   let encoded_query = urlencoding::encode(query);
   let tabs: &[(&str, &str, &str)] = &[
      ("top", "Top", &format!("/search?q={encoded_query}&f=top")),
      ("tweets", "Latest", &format!("/search?q={encoded_query}")),
      (
         "media",
         "Media",
         &format!("/search?q={encoded_query}&f=media"),
      ),
      (
         "users",
         "Users",
         &format!("/search?q={encoded_query}&f=users"),
      ),
   ];
   html! {
       ul class="tab" {
           @for &(id, label, href) in tabs {
               li class=(if active == id { "tab-item active" } else { "tab-item" }) {
                   a href=(href) { (label) }
               }
           }
       }
   }
}

/// Search filter state for form rendering.
#[derive(Default)]
#[expect(
   clippy::struct_excessive_bools,
   reason = "mirrors HTML form checkboxes"
)]
#[expect(
   clippy::module_name_repetitions,
   reason = "SearchFilters is the canonical name"
)]
pub struct SearchFilters {
   pub media:            bool,
   pub images:           bool,
   pub videos:           bool,
   pub links:            bool,
   pub news:             bool,
   pub quote:            bool,
   pub verified:         bool,
   pub exclude_replies:  bool,
   pub exclude_retweets: bool,
   pub since:            String,
   pub until:            String,
   pub min_faves:        String,
}

impl SearchFilters {
   const fn is_panel_open(&self) -> bool {
      self.media
         || self.images
         || self.videos
         || self.links
         || self.news
         || self.quote
         || self.verified
         || self.exclude_replies
         || self.exclude_retweets
         || !self.since.is_empty()
         || !self.until.is_empty()
         || !self.min_faves.is_empty()
   }
}

/// Render search panel with advanced filters.
/// `action` controls the form action URL (default "/search", or
/// "/{user}/search" for profile search).
pub fn render_search_panel_with_action(
   query: &str,
   filters: Option<&SearchFilters>,
   action: &str,
) -> Markup {
   let default_filters = SearchFilters::default();
   let filt = filters.unwrap_or(&default_filters);
   let is_open = filt.is_panel_open();

   html! {
       form method="get" action=(action) class="search-field" autocomplete="off" {
           input type="hidden" name="f" value="tweets";
           div class="pref-group pref-input pref-inline" {
               label for="search-query" class="sr-only" { "Search" }
               input id="search-query" type="text" name="q" value=(query) placeholder="Enter search...";
           }
           button type="submit" aria-label="Search" { span class="icon-search" {} }

           input id="search-panel-toggle" type="checkbox" checked[is_open];
           label for="search-panel-toggle" {
               (icon("down", "", "", "", ""))
           }

           div class="search-panel" {
               // Filter section
               @for prefix in &["filter", "exclude"] {
                   @let cap = {
                       let mut chars = prefix.chars();
                       chars.next().map_or_else(String::new, |ch| format!("{}{}", ch.to_uppercase(), chars.as_str()))
                   };
                   span class="search-title" { (cap) }
                   div class="search-toggles" {
                       @let pfx = &prefix[..1];
                       (gen_search_checkbox(&format!("{pfx}-nativeretweets"), "Retweets", false))
                       (gen_search_checkbox(&format!("{pfx}-media"), "Media", if *prefix == "filter" { filt.media } else { false }))
                       (gen_search_checkbox(&format!("{pfx}-videos"), "Videos", if *prefix == "filter" { filt.videos } else { false }))
                       (gen_search_checkbox(&format!("{pfx}-news"), "News", if *prefix == "filter" { filt.news } else { false }))
                       (gen_search_checkbox(&format!("{pfx}-native_video"), "Native videos", false))
                       (gen_search_checkbox(&format!("{pfx}-replies"), "Replies", if *prefix == "exclude" { filt.exclude_replies } else { false }))
                       (gen_search_checkbox(&format!("{pfx}-links"), "Links", if *prefix == "filter" { filt.links } else { false }))
                       (gen_search_checkbox(&format!("{pfx}-images"), "Images", if *prefix == "filter" { filt.images } else { false }))
                       (gen_search_checkbox(&format!("{pfx}-quote"), "Quotes", if *prefix == "filter" { filt.quote } else { false }))
                       (gen_search_checkbox(&format!("{pfx}-spaces"), "Spaces", false))
                   }
               }

               // Date range and min likes
               div class="search-row" {
                   div {
                       span class="search-title" { "Time range" }
                       div class="date-range" {
                           span class="date-input" {
                               label for="search-since" class="sr-only" { "Since date" }
                               input id="search-since" type="date" name="since" value=(filt.since);
                               span class="icon-container" { span class="icon-calendar" {} }
                           }
                           span class="search-title" { "-" }
                           span class="date-input" {
                               label for="search-until" class="sr-only" { "Until date" }
                               input id="search-until" type="date" name="until" value=(filt.until);
                               span class="icon-container" { span class="icon-calendar" {} }
                           }
                       }
                   }
                   div {
                       span class="search-title" { "Minimum likes" }
                       div class="pref-group pref-input" {
                           label for="search-min-faves" class="sr-only" { "Minimum likes" }
                           input id="search-min-faves" type="number" name="min_faves" value=(filt.min_faves) placeholder="Number...";
                       }
                   }
               }
           }
       }
   }
}

/// Generate a search checkbox.
fn gen_search_checkbox(name: &str, label: &str, checked: bool) -> Markup {
   let id = format!("search-{name}");
   html! {
       label class="pref-group checkbox-container" for=(id) {
           (label)
           input id=(id) type="checkbox" name=(name) value="on" checked[checked];
           span class="checkbox" {}
       }
   }
}

mod urlencoding {
   use crate::utils::formatters;

   pub fn encode(input: &str) -> String {
      formatters::url_encode(input)
   }
}
