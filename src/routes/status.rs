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
      Redirect,
      Response,
   },
   routing::get,
};
use axum_extra::extract::CookieJar;
use maud::html;
use serde::Deserialize;
use tweet_view::{
   TweetRenderer,
   render_reply_chains,
   thread_context,
};

use crate::{
   AppState,
   config::Config,
   cache::{
      keys as cache_keys,
      ttl,
   },
   error::{
      Error,
      Result,
   },
   types::{
      Conversation,
      Prefs,
      Tweet,
   },
   views::{
      embed,
      layout,
      timeline::render_to_top_with_focus,
      tweet as tweet_view,
   },
};

#[derive(Debug, Deserialize)]
pub struct StatusQuery {
   pub cursor: Option<String>,
   pub scroll: Option<String>,
}

pub fn router() -> Router<AppState> {
   let mut router = Router::new();

   for prefix in ["status", "statuses"] {
      router = router
         .route(&format!("/{{username}}/{prefix}/{{id}}"), get(status))
         .route(
            &format!("/{{username}}/{prefix}/{{id}}/photo"),
            get(status_media_redirect),
         )
         .route(
            &format!("/{{username}}/{prefix}/{{id}}/photo/{{idx}}"),
            get(status_media_redirect),
         )
         .route(
            &format!("/{{username}}/{prefix}/{{id}}/video"),
            get(status_media_redirect),
         )
         .route(
            &format!("/{{username}}/{prefix}/{{id}}/video/{{idx}}"),
            get(status_media_redirect),
         )
         .route(
            &format!("/{{username}}/{prefix}/{{id}}/history"),
            get(edit_history),
         );
   }

   router
      .route("/{username}/thread/{id}", get(thread_redirect))
      .route("/i/status/{id}", get(status_by_id))
      .route("/i/web/status/{id}", get(status_by_id))
      .route("/i/user/{id}", get(user_by_id))
}

async fn thread_redirect(Path((username, id)): Path<(String, String)>) -> Response {
   Redirect::to(&format!("/{username}/status/{id}")).into_response()
}

/// Redirect legacy media URLs (photo/video/history) to the main status page.
async fn status_media_redirect(Path((username, id)): Path<(String, String)>) -> Response {
   Redirect::to(&format!("/{username}/status/{id}")).into_response()
}

async fn status(
   State(state): State<AppState>,
   jar: CookieJar,
   Path((username, id)): Path<(String, String)>,
   Query(query): Query<StatusQuery>,
) -> Result<Response> {
   // Extract prefs from cookies
   let prefs = Prefs::from_cookies(&jar, &state.config);

   // Fetch conversation (with cache for first page)
   let conv_result = if query.cursor.is_none() {
      let cache_key = cache_keys::conversation(&id);
      if let Some(cached) = state.cache.get(&cache_key) {
         tracing::debug!("Cache hit for conversation: {id}");
         Ok(cached)
      } else {
         let result = state.api.get_conversation(&id, None).await;
         if let Ok(ref conv) = result {
            state.cache.set(&cache_key, conv, ttl::DEFAULT);
         }
         result
      }
   } else {
      state
         .api
         .get_conversation(&id, query.cursor.as_deref())
         .await
   };

   match conv_result {
      Ok(conversation) => {
         let is_scroll = query.scroll.as_ref().is_some_and(|val| val == "true");
         let has_cursor = query.cursor.is_some();

         // For AJAX scroll requests, return only the replies HTML fragment.
         // Check this first — paginated responses don't include the main tweet.
         if is_scroll {
            if conversation.replies.content.is_empty() {
               return Ok(StatusCode::NOT_FOUND.into_response());
            }
            let content = render_reply_chains(
               &conversation.replies.content,
               conversation.replies.bottom.as_deref().unwrap_or_default(),
               &username,
               &id,
               &state.config,
               &prefs,
            );
            return Ok(Html(content.into_string()).into_response());
         }

         // Paginated (cursor) requests with no replies — the cursor was a
         // dead end. Re-fetch the first page and render it without the bottom
         // cursor so "Load more replies" disappears.
         if has_cursor && conversation.replies.content.is_empty() {
            let cache_key = cache_keys::conversation(&id);
            let mut first_page = if let Some(cached) =
               state.cache.get::<Conversation>(&cache_key)
            {
               cached
            } else {
               match state.api.get_conversation(&id, None).await {
                  Ok(fresh) => {
                     state.cache.set(&cache_key, &fresh, ttl::DEFAULT);
                     fresh
                  },
                  Err(err) => return Err(err),
               }
            };
            first_page.replies.bottom = None;
            return Ok(render_conversation(
               &first_page,
               false,
               &username,
               &id,
               &prefs,
               &state.config,
            ));
         }

         // Paginated responses (with cursor) don't include the main tweet.
         // Recover it from the first-page cache, or re-fetch without cursor.
         #[expect(clippy::shadow_same, reason = "rebinding as mutable")]
         let mut conversation = conversation;
         if conversation.tweet.id == 0 && has_cursor {
            let cache_key = cache_keys::conversation(&id);
            if let Some(cached) = state.cache.get::<Conversation>(&cache_key) {
               conversation.tweet = cached.tweet;
            } else if let Ok(fresh) = state.api.get_conversation(&id, None).await {
               state.cache.set(&cache_key, &fresh, ttl::DEFAULT);
               conversation.tweet = fresh.tweet;
            }
         }

         Ok(render_conversation(
            &conversation,
            has_cursor,
            &username,
            &id,
            &prefs,
            &state.config,
         ))
      },
      Err(Error::TweetNotFound(msg)) => {
         let markup = layout::render_error(&state.config, "Tweet not found", &msg);
         Ok((StatusCode::NOT_FOUND, Html(markup.into_string())).into_response())
      },
      Err(err) => {
         let markup = layout::render_error(&state.config, "Error", &err.to_string());
         Ok((
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(markup.into_string()),
         )
            .into_response())
      },
   }
}

/// Render a conversation page (first page or paginated replies).
fn render_conversation(
   conversation: &Conversation,
   has_cursor: bool,
   username: &str,
   id: &str,
   prefs: &Prefs,
   config: &Config,
) -> Response {
   let tweet = &conversation.tweet;

   // If the main tweet is unavailable (tombstone/withheld), show error
   if !tweet.available && tweet.id == 0 {
      let msg = if tweet.tombstone.is_empty() {
         "Tweet is unavailable"
      } else {
         &tweet.tombstone
      };
      let markup = layout::render_error(config, "Tweet not found", msg);
      return (StatusCode::NOT_FOUND, Html(markup.into_string())).into_response();
   }

   let content = html! {
       div class="conversation" {
           @if has_cursor {
               // Paginated replies page: compact tweet header + replies only
               div class="main-thread" {
                   div class="main-tweet" id="m" {
                       (TweetRenderer::new(tweet, config, false).prefs(prefs).render())
                   }
               }
               div class="timeline-item show-more" {
                   a href=(format!("/{username}/status/{id}#r")) { "Back to tweet" }
               }
               div class="replies" id="r" {
                   (render_reply_chains(
                       &conversation.replies.content,
                       conversation.replies.bottom.as_deref().unwrap_or_default(),
                       username,
                       id,
                       config,
                       prefs,
                   ))
               }
           } @else {
               div class="main-thread" {
                   // Before context (parent tweets) with thread line
                   @if !conversation.before.content.is_empty() {
                       div class="before-tweet thread-line" {
                           // "Earlier replies" link when thread doesn't start at root
                           @let first = &conversation.before.content[0];
                           @if tweet.thread_id != first.id && (first.reply_id > 0 || !first.available) {
                               div class="timeline-item more-replies earlier-replies" {
                                   a class="more-replies-text" href=(format!("/{}/status/{}#m", first.user.username, first.id)) {
                                       "earlier replies"
                                   }
                               }
                           }
                           @let before_len = conversation.before.content.len();
                           @for (idx, tw) in conversation.before.content.iter().enumerate() {
                               @let ctx = thread_context(idx, before_len, false);
                               (TweetRenderer::new(tw, config, false).prefs(prefs).thread_ctx(ctx).render())
                           }
                       }
                   }

                   // Main tweet (highlighted, larger)
                   @let has_after = !conversation.after.content.is_empty();
                   @let after_class = if has_after { "thread thread-line" } else { "" };
                   div class="main-tweet" id="m" {
                       (TweetRenderer::new(tweet, config, true).prefs(prefs).extra_class(after_class).render())
                   }

                   // After context (thread continuation) with thread line
                   @if has_after {
                       div class="after-tweet thread-line" {
                           @let after_len = conversation.after.content.len();
                           @let has_more = conversation.after.has_more;
                           @for (idx, tw) in conversation.after.content.iter().enumerate() {
                               @let is_last = idx == after_len - 1 && !has_more;
                               @let ctx = thread_context(idx, after_len, is_last);
                               (TweetRenderer::new(tw, config, false).prefs(prefs).thread_ctx(ctx).render())
                           }
                           @if has_more {
                               @if let Some(last_after) = conversation.after.content.last() {
                                   div class="timeline-item more-replies" {
                                       @if last_after.available {
                                           a class="more-replies-text" href=(format!("/{}/status/{}#m", last_after.user.username, last_after.id)) {
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

               // Replies section
               @if !prefs.hide_replies && !conversation.replies.content.is_empty() {
                   div class="replies" id="r" {
                       (render_reply_chains(
                           &conversation.replies.content,
                           conversation.replies.bottom.as_deref().unwrap_or_default(),
                           username,
                           id,
                           config,
                           prefs,
                       ))
                   }
               }
           }

           // Scroll to top button
           (render_to_top_with_focus("#m"))
       }
   };

   let markup = embed::render_status_page(tweet, &content, prefs, config, username, id);
   Html(markup.into_string()).into_response()
}

async fn status_by_id(
   State(state): State<AppState>,
   Path(id): Path<String>,
   Query(_query): Query<StatusQuery>,
) -> Result<Response> {
   // Validate tweet ID is numeric
   let Ok(tweet_id) = id.parse() else {
      return Ok((
         StatusCode::NOT_FOUND,
         Html(layout::render_error(&state.config, "Not Found", "Invalid tweet ID").into_string()),
      )
         .into_response());
   };
   let cache_key = cache_keys::tweet(tweet_id);

   let tweet_result = if let Some(cached) = state.cache.get::<Tweet>(&cache_key) {
      tracing::debug!("Cache hit for tweet: {id}");
      Ok(cached)
   } else {
      let result = state.api.get_tweet(&id).await;
      if let Ok(ref tweet) = result {
         state.cache.set(&cache_key, tweet, ttl::DEFAULT);
      }
      result
   };

   match tweet_result {
      Ok(tweet) => {
         let redirect_url = format!("/{}/status/{id}", tweet.user.username);
         Ok(Redirect::to(&redirect_url).into_response())
      },
      Err(err) => {
         let markup = layout::render_error(&state.config, "Tweet not found", &err.to_string());
         Ok((StatusCode::NOT_FOUND, Html(markup.into_string())).into_response())
      },
   }
}

/// Lookup user by ID and redirect to their profile.
async fn user_by_id(State(state): State<AppState>, Path(id): Path<String>) -> Result<Response> {
   // Check cache first for ID -> username mapping
   let cache_key = cache_keys::user_id(&id);
   if let Some(username) = state.cache.get::<String>(&cache_key) {
      tracing::debug!("Cache hit for user ID: {id} -> {username}");
      return Ok(Redirect::to(&format!("/{username}")).into_response());
   }

   // Fetch user from API by ID
   match state.api.get_user_by_id(&id).await {
      Ok(user) => {
         // Cache the ID -> username mapping (long TTL since IDs don't change)
         state
            .cache
            .set(&cache_key, &user.username, ttl::USER_ID_MAPPING);
         Ok(Redirect::to(&format!("/{}", user.username)).into_response())
      },
      Err(Error::UserNotFound(msg)) => {
         let content = html! {
             div class="error-page" {
                 h1 { "User Not Found" }
                 p { (msg) }
                 p class="user-id" { "User ID: " (id) }
             }
         };
         let markup = layout::PageLayout::new(&state.config, "User Not Found", content).render();
         Ok((StatusCode::NOT_FOUND, Html(markup.into_string())).into_response())
      },
      Err(err) => {
         let markup = layout::render_error(&state.config, "Error", &err.to_string());
         Ok((
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(markup.into_string()),
         )
            .into_response())
      },
   }
}

/// Edit history page handler.
async fn edit_history(
   State(state): State<AppState>,
   jar: CookieJar,
   Path((username, id)): Path<(String, String)>,
) -> Result<Response> {
   // Validate tweet ID
   if id.len() > 19 || !id.chars().all(|ch| ch.is_ascii_digit()) {
      let markup = layout::render_error(&state.config, "Not Found", "Invalid tweet ID");
      return Ok((StatusCode::NOT_FOUND, Html(markup.into_string())).into_response());
   }

   let prefs = Prefs::from_cookies(&jar, &state.config);

   if let Ok(edits) = state.api.get_edit_history(&id).await {
      let title = format!(
         "Edit History: {} (@{})",
         edits.latest.text.chars().take(40).collect::<String>(),
         if edits.latest.user.username.is_empty() {
            &username
         } else {
            &edits.latest.user.username
         }
      );

      let content = html! {
          div class="edit-history" {
              div class="latest-edit" {
                  div class="edit-history-header" { "Latest post" }
                  (TweetRenderer::new(&edits.latest, &state.config, false)
                     .prefs(&prefs)
                     .render())
              }
              @if !edits.history.is_empty() {
                  div class="previous-edits" {
                      div class="edit-history-header" { "Version history" }
                      @for tweet in &edits.history {
                          div class="tweet-edit" {
                              (TweetRenderer::new(tweet, &state.config, false)
                                 .prefs(&prefs)
                                 .render())
                          }
                      }
                  }
              }
          }
      };

      let markup = layout::PageLayout::new(&state.config, &title, content)
         .prefs(&prefs)
         .render();
      Ok(Html(markup.into_string()).into_response())
   } else {
      let markup = layout::render_error(&state.config, "Not Found", "Tweet edit history not found");
      Ok((StatusCode::NOT_FOUND, Html(markup.into_string())).into_response())
   }
}
