use axum::{
   Router,
   extract::{
      Path,
      Query as AxumQuery,
      RawQuery,
      State,
   },
   http::{
      StatusCode,
      header::CONTENT_TYPE,
   },
   response::{
      Html,
      IntoResponse as _,
      Redirect,
      Response,
   },
   routing::get,
};
use axum_extra::extract::CookieJar;
use serde::Deserialize;

use crate::{
   AppState,
   error::Result,
   types::{
      Prefs,
      Query,
      QueryKind,
   },
   views::{
      layout,
      search as search_view,
   },
};

#[derive(Debug, Default, Deserialize)]
pub struct SearchQuery {
   #[serde(rename = "q")]
   pub query:      Option<String>,
   #[serde(rename = "f")]
   pub filter:     Option<String>,
   pub cursor:     Option<String>,
   // Filter parameters
   pub from:       Option<String>,
   pub since:      Option<String>,
   pub until:      Option<String>,
   pub min_faves:  Option<String>,
   // Filter toggles (f-media=on, e-replies=on, etc.)
   #[serde(rename = "f-media")]
   pub f_media:    Option<String>,
   #[serde(rename = "f-images")]
   pub f_images:   Option<String>,
   #[serde(rename = "f-videos")]
   pub f_videos:   Option<String>,
   #[serde(rename = "f-links")]
   pub f_links:    Option<String>,
   #[serde(rename = "f-news")]
   pub f_news:     Option<String>,
   #[serde(rename = "f-quote")]
   pub f_quote:    Option<String>,
   #[serde(rename = "f-verified")]
   pub f_verified: Option<String>,
   #[serde(rename = "e-replies")]
   pub e_replies:  Option<String>,
   #[serde(rename = "e-retweets")]
   pub e_retweets: Option<String>,
}

impl SearchQuery {
   /// Convert URL parameters to `SearchFilters` for form rendering.
   pub fn to_filters(&self) -> search_view::SearchFilters {
      search_view::SearchFilters {
         media:            self.f_media.as_deref() == Some("on"),
         images:           self.f_images.as_deref() == Some("on"),
         videos:           self.f_videos.as_deref() == Some("on"),
         links:            self.f_links.as_deref() == Some("on"),
         news:             self.f_news.as_deref() == Some("on"),
         quote:            self.f_quote.as_deref() == Some("on"),
         verified:         self.f_verified.as_deref() == Some("on"),
         exclude_replies:  self.e_replies.as_deref() == Some("on"),
         exclude_retweets: self.e_retweets.as_deref() == Some("on"),
         since:            self.since.clone().unwrap_or_default(),
         until:            self.until.clone().unwrap_or_default(),
         min_faves:        self.min_faves.clone().unwrap_or_default(),
      }
   }

   /// Convert URL parameters to `Query` struct.
   fn to_query(&self) -> Query {
      let raw_query = self.query.as_deref().unwrap_or("");

      // Determine query kind from 'f' parameter
      let kind = match self.filter.as_deref() {
         Some("replies") => QueryKind::Replies,
         Some("media") => QueryKind::Media,
         Some("users") => QueryKind::Users,
         _ => QueryKind::Posts,
      };

      // Parse the raw query text for inline filters
      let mut query = Query::parse(raw_query, kind);

      // Add from user if specified as parameter
      if let Some(ref from) = self.from {
         for user in from.split(',') {
            let user = user.trim();
            if !user.is_empty() && !query.from_user.contains(&user.to_owned()) {
               query.from_user.push(user.to_owned());
            }
         }
      }

      // Add date filters from parameters
      if let Some(ref since) = self.since
         && query.since.is_empty()
      {
         query.since.clone_from(since);
      }
      if let Some(ref until) = self.until
         && query.until.is_empty()
      {
         query.until.clone_from(until);
      }
      if let Some(ref min) = self.min_faves
         && query.min_likes.is_empty()
      {
         query.min_likes.clone_from(min);
      }

      // Add filter toggles (data-driven to avoid repetition)
      let filter_toggles: &[(&Option<String>, &str)] = &[
         (&self.f_media, "media"),
         (&self.f_images, "images"),
         (&self.f_videos, "videos"),
         (&self.f_links, "links"),
         (&self.f_news, "news"),
         (&self.f_quote, "quote"),
         (&self.f_verified, "verified"),
      ];
      for &(param, name) in filter_toggles {
         if param.as_deref() == Some("on") && !query.filters.iter().any(|filter| filter == name) {
            query.filters.push((*name).to_owned());
         }
      }

      // Add exclude toggles
      let exclude_toggles: &[(&Option<String>, &str)] =
         &[(&self.e_replies, "replies"), (&self.e_retweets, "retweets")];
      for &(param, name) in exclude_toggles {
         if param.as_deref() == Some("on") && !query.excludes.iter().any(|excl| excl == name) {
            query.excludes.push((*name).to_owned());
         }
      }

      query
   }
}

pub fn router() -> Router<AppState> {
   Router::new()
      .route("/search", get(search))
      .route("/hashtag/{tag}", get(hashtag))
      .route("/opensearch", get(opensearch))
}

async fn search(
   State(state): State<AppState>,
   jar: CookieJar,
   RawQuery(raw_qs): RawQuery,
   AxumQuery(params): AxumQuery<SearchQuery>,
) -> Result<Response> {
   // Strip empty query parameters (e.g. since=&until=&min_faves=) for clean URLs
   if let Some(ref qs) = raw_qs {
      let clean: Vec<&str> = qs
         .split('&')
         .filter(|pair| {
            pair.split_once('=')
               .is_none_or(|(_, val)| !val.is_empty())
         })
         .collect();
      if clean.len() < qs.split('&').count() {
         let url = if clean.is_empty() {
            "/search".to_owned()
         } else {
            format!("/search?{}", clean.join("&"))
         };
         return Ok(Redirect::to(&url).into_response());
      }
   }

   // Extract prefs from cookies
   let prefs = Prefs::from_cookies(&jar, &state.config);

   let raw_q = params.query.clone().unwrap_or_default();

   // Redirect comma-separated usernames to multi-user timeline
   if raw_q.contains(',')
      && raw_q.split(',').all(|segment| {
         let trimmed = segment.trim();
         !trimmed.is_empty()
            && trimmed
               .chars()
               .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '@')
      })
   {
      let cleaned = raw_q
         .split(',')
         .map(|segment| segment.trim().trim_start_matches('@'))
         .collect::<Vec<_>>()
         .join(",");
      return Ok(Redirect::to(&format!("/{cleaned}")).into_response());
   }

   // Check if this is a user search
   let is_user_search = params.filter.as_deref() == Some("users");

   // Handle empty query - show search UI without calling API
   if raw_q.is_empty() && params.from.is_none() {
      let filters = params.to_filters();
      if is_user_search {
         let content = search_view::render_user_search_results(
            &raw_q,
            &[],
            &state.config,
            None,
            None,
            Some(&prefs),
         );
         let markup = layout::PageLayout::new(&state.config, "Search", content)
            .prefs(&prefs)
            .render();
         return Ok(Html(markup.into_string()).into_response());
      }
      let empty_tweets = Vec::new();
      let content = search_view::render_search_results_with_prefs(
         &raw_q,
         &empty_tweets,
         &state.config,
         None,
         Some(&prefs),
         Some(&filters),
         None,
      );
      let markup = layout::PageLayout::new(&state.config, "Search", content)
         .prefs(&prefs)
         .render();
      return Ok(Html(markup.into_string()).into_response());
   }

   if is_user_search {
      // User search
      let search_result = state
         .api
         .search_users(&raw_q, params.cursor.as_deref())
         .await;

      match search_result {
         Ok(result) => {
            let cursor = result.bottom.as_deref();

            let newer_url = params.cursor.is_some().then(|| {
               format!(
                  "/search?q={}&f=users",
                  percent_encoding::utf8_percent_encode(&raw_q, percent_encoding::NON_ALPHANUMERIC)
               )
            });
            let content = search_view::render_user_search_results(
               &raw_q,
               &result.content,
               &state.config,
               cursor,
               newer_url.as_deref(),
               Some(&prefs),
            );
            let title = format!("Search ({raw_q}) | Users");
            let canonical = format!(
               "https://x.com/search?q={}&src=typed_query",
               percent_encoding::utf8_percent_encode(&raw_q, percent_encoding::NON_ALPHANUMERIC)
            );
            let referer = format!(
               "/search?q={}&f=users",
               percent_encoding::utf8_percent_encode(&raw_q, percent_encoding::NON_ALPHANUMERIC)
            );
            let markup = layout::PageLayout::new(&state.config, &title, content)
               .prefs(&prefs)
               .canonical(&canonical)
               .referer(&referer)
               .render();
            Ok(Html(markup.into_string()).into_response())
         },
         Err(err) => {
            let markup = layout::render_error(&state.config, "Search Error", &err.to_string());
            Ok((
               StatusCode::INTERNAL_SERVER_ERROR,
               Html(markup.into_string()),
            )
               .into_response())
         },
      }
   } else {
      // Tweet search
      // Parse query with filters
      let query = params.to_query();

      // Build the actual search query for Twitter API
      let api_query = query.build();

      // Execute search
      let search_result = state.api.search(&api_query, params.cursor.as_deref()).await;

      match search_result {
         Ok(timeline) => {
            let tweets = timeline.content.into_iter().flatten().collect::<Vec<_>>();
            let cursor = timeline.bottom.as_deref();

            // Display the original user query, not the API query
            let display_query = if query.has_filters() {
               &api_query
            } else {
               &raw_q
            };

            let filters = params.to_filters();
            let newer_url = params.cursor.is_some().then(|| {
               format!(
                  "/search?q={}",
                  percent_encoding::utf8_percent_encode(&raw_q, percent_encoding::NON_ALPHANUMERIC)
               )
            });
            let content = search_view::render_search_results_with_prefs(
               display_query,
               &tweets,
               &state.config,
               cursor,
               Some(&prefs),
               Some(&filters),
               newer_url.as_deref(),
            );
            let title = format!("Search ({raw_q})");
            let canonical = format!(
               "https://x.com/search?f=live&q={}&src=typed_query",
               percent_encoding::utf8_percent_encode(&raw_q, percent_encoding::NON_ALPHANUMERIC)
            );
            let rss_url = format!(
               "/search/rss?f=tweets&q={}",
               percent_encoding::utf8_percent_encode(&raw_q, percent_encoding::NON_ALPHANUMERIC)
            );
            let referer = format!(
               "/search?q={}",
               percent_encoding::utf8_percent_encode(&raw_q, percent_encoding::NON_ALPHANUMERIC)
            );
            let markup = layout::PageLayout::new(&state.config, &title, content)
               .prefs(&prefs)
               .rss(&rss_url)
               .canonical(&canonical)
               .referer(&referer)
               .render();
            Ok(Html(markup.into_string()).into_response())
         },
         Err(err) => {
            let markup = layout::render_error(&state.config, "Search Error", &err.to_string());
            Ok((
               StatusCode::INTERNAL_SERVER_ERROR,
               Html(markup.into_string()),
            )
               .into_response())
         },
      }
   }
}

async fn hashtag(
   State(state): State<AppState>,
   jar: CookieJar,
   Path(tag): Path<String>,
   AxumQuery(query): AxumQuery<SearchQuery>,
) -> Result<Response> {
   // Search for the hashtag
   let hashtag_query = format!("#{tag}");

   search(
      State(state),
      jar,
      RawQuery(None),
      AxumQuery(SearchQuery {
         query: Some(hashtag_query),
         filter: query.filter,
         cursor: query.cursor,
         ..Default::default()
      }),
   )
   .await
}

async fn opensearch(State(state): State<AppState>) -> Response {
   let xml = format!(
      r#"<?xml version="1.0" encoding="UTF-8"?>
<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
  <ShortName>{}</ShortName>
  <Description>Twitter search via {}</Description>
  <InputEncoding>UTF-8</InputEncoding>
  <Url type="text/html" template="{}/search?q={{searchTerms}}"/>
</OpenSearchDescription>"#,
      state.config.server.title,
      state.config.server.hostname,
      state.config.url_prefix()
   );

   (
      [(CONTENT_TYPE, "application/opensearchdescription+xml")],
      xml,
   )
      .into_response()
}
