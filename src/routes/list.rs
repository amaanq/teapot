use axum::{
   Router,
   extract::{
      Path,
      Query,
      State,
   },
   http::StatusCode,
   response::{
      Html,
      IntoResponse as _,
      Response,
   },
   routing::get,
};
use axum_extra::extract::CookieJar;
use maud::html;
use serde::Deserialize;

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
   types::{
      List,
      PaginatedResult,
      Prefs,
      User,
   },
   views::{
      layout,
      timeline as timeline_view,
      user_list,
   },
};

#[derive(Debug, Deserialize)]
pub struct ListQuery {
   pub cursor:       Option<String>,
   /// Exclude retweets from list timeline (?no_retweets=true).
   pub no_retweets:  Option<String>,
}

pub fn router() -> Router<AppState> {
   Router::new()
      .route("/{username}/lists", get(user_lists_unsupported))
      .route("/{username}/lists/", get(user_lists_unsupported))
      .route("/{username}/lists/{slug}", get(list_by_slug))
      .route("/i/lists/{id}", get(list_by_id))
      .route("/i/lists/{id}/members", get(list_members))
}

/// User lists browsing is not supported.
async fn user_lists_unsupported(
   State(state): State<AppState>,
   jar: CookieJar,
   Path(username): Path<String>,
) -> Response {
   let prefs = Prefs::from_cookies(&jar, &state.config);
   let content = html! {
       div class="overlay-panel" {
           h1 { "User Lists" }
           p {
               "Browsing user lists is not currently supported. "
               "You can access a specific list directly if you have its URL or slug."
           }
           p {
               a href=(format!("/{username}")) { "Go to @" (username) "'s profile" }
           }
       }
   };

   let title = format!("@{username}'s Lists");
   let markup = layout::PageLayout::new(&state.config, &title, content)
      .prefs(&prefs)
      .render();
   Html(markup.into_string()).into_response()
}

async fn list_by_slug(
   State(state): State<AppState>,
   jar: CookieJar,
   Path((username, slug)): Path<(String, String)>,
   Query(query): Query<ListQuery>,
) -> Result<Response> {
   let prefs = Prefs::from_cookies(&jar, &state.config);

   // Look up list by owner username and slug
   let list = state.api.get_list_by_slug(&username, &slug).await?;

   // Fetch list tweets with cursor support
   let timeline = state
      .api
      .get_list_tweets(&list.id, query.cursor.as_deref())
      .await?;
   let exclude_rts = query.no_retweets.as_deref() == Some("true");
   let groups = if exclude_rts {
      timeline
         .content
         .into_iter()
         .filter(|group| !group.first().is_some_and(|t| t.retweet.is_some()))
         .collect()
   } else {
      timeline.content
   };
   let cursor = timeline.bottom.as_deref();
   let base_url = format!("/{username}/lists/{slug}");

   let content = html! {
       div class="timeline-container" {
           (timeline_view::render_list_header(&list, "tweets", &state.config))
           (timeline_view::render_timeline(&groups, &state.config, cursor, Some(&base_url)))
       }
   };

   let title = format!("{} - List by @{}", list.name, username);
   let markup = layout::PageLayout::new(&state.config, &title, content)
      .description(&list.description)
      .prefs(&prefs)
      .render();
   Ok(Html(markup.into_string()).into_response())
}

async fn list_by_id(
   State(state): State<AppState>,
   jar: CookieJar,
   Path(id): Path<String>,
   Query(query): Query<ListQuery>,
) -> Result<Response> {
   let prefs = Prefs::from_cookies(&jar, &state.config);

   let (list, timeline) = tokio::join!(
      async {
         let cache_key = cache_keys::list(&id);
         if let Some(cached) = state.cache.get::<List>(&cache_key) {
            tracing::debug!("Cache hit for list: {id}");
            return Ok::<_, Error>(cached);
         }
         let fetched = state.api.get_list(&id).await?;
         state.cache.set(&cache_key, &fetched, ttl::DEFAULT);
         Ok(fetched)
      },
      state.api.get_list_tweets(&id, query.cursor.as_deref()),
   );
   let list = list?;
   let timeline = timeline?;
   let exclude_rts = query.no_retweets.as_deref() == Some("true");
   let groups = if exclude_rts {
      timeline
         .content
         .into_iter()
         .filter(|group| !group.first().is_some_and(|t| t.retweet.is_some()))
         .collect()
   } else {
      timeline.content
   };
   let cursor = timeline.bottom.as_deref();
   let base_url = format!("/i/lists/{id}");

   let content = html! {
       div class="timeline-container" {
           (timeline_view::render_list_header(&list, "tweets", &state.config))
           (timeline_view::render_timeline(&groups, &state.config, cursor, Some(&base_url)))
       }
   };

   let title = format!("{} - List", list.name);
   let markup = layout::PageLayout::new(&state.config, &title, content)
      .description(&list.description)
      .prefs(&prefs)
      .render();
   Ok(Html(markup.into_string()).into_response())
}

async fn list_members(
   State(state): State<AppState>,
   jar: CookieJar,
   Path(id): Path<String>,
   Query(query): Query<ListQuery>,
) -> Result<Response> {
   let prefs = Prefs::from_cookies(&jar, &state.config);

   let (list, members_result) = tokio::join!(
      async {
         let cache_key = cache_keys::list(&id);
         if let Some(cached) = state.cache.get::<List>(&cache_key) {
            return Ok::<_, Error>(cached);
         }
         let fetched = state.api.get_list(&id).await?;
         state.cache.set(&cache_key, &fetched, ttl::DEFAULT);
         Ok(fetched)
      },
      async {
         let members_cache_key = cache_keys::list_members(&id);
         if query.cursor.is_none() {
            if let Some(cached) = state.cache.get::<PaginatedResult<User>>(&members_cache_key) {
               tracing::debug!("Cache hit for list members: {id}");
               return Ok(cached);
            }
            let result = state.api.get_list_members(&id, None).await;
            if let Ok(ref members) = result {
               state.cache.set(&members_cache_key, members, ttl::DEFAULT);
            }
            result
         } else {
            state
               .api
               .get_list_members(&id, query.cursor.as_deref())
               .await
         }
      },
   );
   let list = list?;

   match members_result {
      Ok(members) => {
         let cursor = members.bottom.as_deref();
         let base_url = format!("/i/lists/{id}/members");

         let content = html! {
             div class="timeline-container" {
                 (timeline_view::render_list_header(&list, "members", &state.config))
                 (user_list::render_user_list(&members.content, &state.config, cursor, Some(&base_url), Some(&prefs)))
             }
         };

         let title = format!("{} - Members", list.name);
         let markup = layout::PageLayout::new(&state.config, &title, content)
            .prefs(&prefs)
            .render();
         Ok(Html(markup.into_string()).into_response())
      },
      Err(err) => {
         let markup =
            layout::render_error(&state.config, "Error loading members", &err.to_string());
         Ok((
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(markup.into_string()),
         )
            .into_response())
      },
   }
}

// List header rendering lives in views::timeline::render_list_header
