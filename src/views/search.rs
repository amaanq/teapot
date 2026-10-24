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
   routes::feed::{
      Feed,
      load_more,
      tweet_ids,
      user_ids,
   },
   types::{
      prefs::Prefs,
      query::QueryKind,
      tweet::Tweet,
      user::User,
   },
   utils::formatters,
   views::{
      renderutils::{
         config,
         icon,
      },
      tweet::tweet_item,
      user_list::user_item,
   },
};

/// Render the empty search page (home search bar).
#[component]
pub async fn landing_search(cx: &Cx) -> Result<impl View> {
   Ok(view! {
      <section class="landing-page">
         <div class="landing-brand">
            <img class="landing-logo" src="/logo.svg" alt="">
            <span class="landing-name">(&config(cx).server.title)</span>
         </div>
         <div class="landing-search">
            <form method="get" action="/search" autocomplete="off" role="search">
               <input type="hidden" name="f" value="tweets">
               <label for="home-search" class="sr-only">"Search"</label>
               <span class="icon-search landing-search-icon" aria-hidden="true"></span>
               <input
                  id="home-search"
                  type="text"
                  name="q"
                  autofocus=""
                  placeholder="Search"
                  dir="auto"
               >
               <button type="submit">"Search"</button>
            </form>
         </div>
      </section>
   })
}

/// Render search results page with prefs support.
#[component]
pub async fn search_results<'a>(
   query: &'a str,
   tweets: &'a [Tweet],
   prefs: &'a Prefs,
   active_tab: QueryKind,
   #[default] feed: Option<&'a Feed>,
   #[default] cursor: Option<&'a str>,
   #[default] filters: Option<&'a SearchFilters>,
   #[default] newer_url: Option<&'a str>,
) -> Result<impl View> {
   Ok(view! {
      <div class="timeline-container search-results">
         <div class="timeline-header">
            search_panel(query: query, filters: filters, action: "/search")
         </div>
         search_tabs(query: query, active: active_tab)
         <div class="timeline">
            // "Load newest" link when viewing paginated results
            if let Some(url) = newer_url {
               <div class="timeline-item show-more"><a href=(url)>"Load newest"</a></div>
            }
            if tweets.is_empty() {
               <div class="search-empty">"No posts found"</div>
            } else {
               #[key(tweet.id)]
               for tweet in tweets {
                  tweet_item(tweet: tweet, prefs: prefs)
               }
               if let Some(feed) = feed {
                  load_more(feed: feed, cursor: cursor, seen: tweet_ids(tweets))
               }
            }
         </div>
      </div>
   })
}

/// Render user search results page.
#[component]
pub async fn user_search_results<'a>(
   query: &'a str,
   users: &'a [User],
   prefs: &'a Prefs,
   #[default] cursor: Option<&'a str>,
   #[default] newer_url: Option<&'a str>,
) -> Result<impl View> {
   let feed = Feed::Users(query.to_owned());
   Ok(view! {
      <div class="timeline-container search-results">
         <div class="timeline-header">
            <form method="get" action="/search" class="search-field user-search-field" autocomplete="off">
               <input type="hidden" name="f" value="users">
               <div class="pref-group pref-input pref-inline">
                  <input type="text" name="q" value=(query) placeholder="Enter username...">
               </div>
               <button type="submit"><span class="icon-search"></span></button>
            </form>
         </div>
         search_tabs(query: query, active: QueryKind::Users)
         <div class="timeline">
            // "Load newest" link when viewing paginated results
            if let Some(url) = newer_url {
               <div class="timeline-item show-more"><a href=(url)>"Load newest"</a></div>
            }
            if users.is_empty() {
               <div class="search-empty">"No people found"</div>
            } else {
               for user in users {
                  user_item(user: user, prefs: prefs)
               }
               load_more(feed: &feed, cursor: cursor, seen: user_ids(users))
            }
         </div>
      </div>
   })
}

const SEARCH_TABS: [(QueryKind, &str, &str); 4] = [
   (QueryKind::Top, "Top", "&f=top"),
   (QueryKind::Posts, "Latest", ""),
   (QueryKind::Media, "Media", "&f=media"),
   (QueryKind::Users, "Users", "&f=users"),
];

/// Render search tabs (Top / Latest / Media / Users).
#[component]
async fn search_tabs(query: &str, active: QueryKind) -> Result<impl View> {
   let encoded_query = formatters::url_encode(query);
   Ok(view! {
      <ul class="tab">
         for (kind, label, filter) in SEARCH_TABS {
            <li class=(class!("tab-item", "active" if active.tab() == kind))>
               <a href=(format!("/search?q={encoded_query}{filter}"))>(label)</a>
            </li>
         }
      </ul>
   })
}

/// Search filter state for form rendering.
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

static NO_FILTERS: SearchFilters = SearchFilters {
   media:            false,
   images:           false,
   videos:           false,
   links:            false,
   news:             false,
   quote:            false,
   verified:         false,
   exclude_replies:  false,
   exclude_retweets: false,
   since:            String::new(),
   until:            String::new(),
   min_faves:        String::new(),
};

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

   /// Whether the checkbox for `filter` starts checked in the include
   /// (`f`) or exclude (`e`) group.
   const fn checked(&self, group: char, filter: &str) -> bool {
      match (group, filter.as_bytes()) {
         ('f', b"media") => self.media,
         ('f', b"videos") => self.videos,
         ('f', b"news") => self.news,
         ('f', b"links") => self.links,
         ('f', b"images") => self.images,
         ('f', b"quote") => self.quote,
         ('e', b"replies") => self.exclude_replies,
         _ => false,
      }
   }
}

const FILTER_GROUPS: [(char, &str, &str); 2] = [
   ('f', "Include", "Only show posts containing:"),
   ('e', "Exclude", "Hide posts containing:"),
];

const FILTERS: [(&str, &str); 10] = [
   ("nativeretweets", "Retweets"),
   ("media", "Media"),
   ("videos", "Videos"),
   ("news", "News"),
   ("native_video", "Native videos"),
   ("replies", "Replies"),
   ("links", "Links"),
   ("images", "Images"),
   ("quote", "Quotes"),
   ("spaces", "Spaces"),
];

/// Render search panel with advanced filters.
/// `action` controls the form action URL (default "/search", or
/// "/{user}/search" for profile search).
#[component]
pub async fn search_panel<'a>(
   query: &'a str,
   action: &'a str,
   #[default] filters: Option<&'a SearchFilters>,
) -> Result<impl View> {
   let filters = filters.unwrap_or(&NO_FILTERS);

   Ok(view! {
      <form method="get" action=(action) class="search-field" autocomplete="off">
         <input type="hidden" name="f" value="tweets">
         <div class="pref-group pref-input pref-inline">
            <label for="search-query" class="sr-only">"Search"</label>
            <input id="search-query" type="text" name="q" value=(query) placeholder="Enter search...">
         </div>
         <button type="submit" aria-label="Search"><span class="icon-search"></span></button>
         <input id="search-panel-toggle" type="checkbox" checked=(filters.is_panel_open())>
         <label for="search-panel-toggle">icon(name: "down")</label>
         <div class="search-panel">
            <div class="search-filter-groups">
               for (group, legend, hint) in FILTER_GROUPS {
                  <fieldset class="search-filter-group">
                     <legend>(legend)</legend>
                     <p>(hint)</p>
                     <div class="search-toggles">
                        for (filter, label) in FILTERS {
                           let id = format!("search-{group}-{filter}");
                           <label class="pref-group checkbox-container" for=(&id)>
                              (label)
                              <input
                                 id=(&id)
                                 type="checkbox"
                                 name=(format!("{group}-{filter}"))
                                 value="on"
                                 checked=(filters.checked(group, filter))
                              >
                              <span class="checkbox"></span>
                           </label>
                        }
                     </div>
                  </fieldset>
               }
            </div>
            // Date range and min likes
            <div class="search-row">
               <div>
                  <span class="search-title">"Time range"</span>
                  <div class="date-range">
                     <span class="date-input">
                        <label for="search-since" class="sr-only">"Since date"</label>
                        <input id="search-since" type="date" name="since" value=(&filters.since)>
                        <span class="icon-container"><span class="icon-calendar"></span></span>
                     </span>
                     <span class="search-title">"-"</span>
                     <span class="date-input">
                        <label for="search-until" class="sr-only">"Until date"</label>
                        <input id="search-until" type="date" name="until" value=(&filters.until)>
                        <span class="icon-container"><span class="icon-calendar"></span></span>
                     </span>
                  </div>
               </div>
               <div>
                  <span class="search-title">"Minimum likes"</span>
                  <div class="pref-group pref-input">
                     <label for="search-min-faves" class="sr-only">"Minimum likes"</label>
                     <input
                        id="search-min-faves"
                        type="number"
                        name="min_faves"
                        value=(&filters.min_faves)
                        placeholder="Number..."
                     >
                  </div>
               </div>
            </div>
         </div>
      </form>
   })
}
