#![expect(
   clippy::same_name_method,
   clippy::shadow_same,
   clippy::shadow_unrelated,
   reason = "topcoat's #[page] and view! expansions"
)]

use topcoat::{
   context::Cx,
   router::{
      page,
      path_param,
      query_params,
   },
   view::{
      View,
      view,
   },
};

use crate::{
   AppState,
   cache::keys as cache_keys,
   error::Result,
   routes::helpers::{
      request_prefs,
      state,
   },
   types::{
      timeline::{
         List,
         PaginatedResult,
      },
      tweet::Tweet,
      user::User,
   },
   views::{
      document::document,
      timeline::{
         ListTab,
         list_header,
         timeline,
      },
      user_list::user_list,
   },
};

#[query_params(error = bad_request)]
struct ListQuery {
   cursor:      Option<String>,
   /// Exclude retweets from list timeline (`?no_retweets=true`).
   no_retweets: Option<String>,
}

path_param!(username);
path_param!(slug);
path_param!(id);

/// User lists browsing is not supported.
#[page("/{username}/lists")]
pub async fn user_lists_unsupported(cx: &Cx) -> topcoat::Result<impl View> {
   let username = path_param::<Username>(cx);
   let prefs = request_prefs(cx);
   let title = format!("@{username}'s Lists");
   Ok(view! {
      document(
         title: &title,
         prefs: &prefs,
         <div class="overlay-panel">
            <h1>"User Lists"</h1>
            <p>
               "Browsing user lists is not currently supported. "
               "You can access a specific list directly if you have its URL or slug."
            </p>
            <p><a href=(format!("/{username}"))>"Go to @" (username) "'s profile"</a></p>
         </div>
      )
   })
}

/// Filter retweets from timeline groups if requested.
fn maybe_filter_retweets(content: Vec<Vec<Tweet>>, exclude: bool) -> Vec<Vec<Tweet>> {
   if exclude {
      content
         .into_iter()
         .filter(|group| group.first().is_none_or(|tweet| tweet.retweet.is_none()))
         .collect()
   } else {
      content
   }
}

async fn cached_list(state: &AppState, id: &str) -> Result<List> {
   let cache_key = cache_keys::list(id);
   if let Some(cached) = state.cache.get::<List>(&cache_key) {
      tracing::debug!("Cache hit for list: {id}");
      return Ok(cached);
   }
   let fetched = state.api.get_list(id).await?;
   state
      .cache
      .set(&cache_key, &fetched, state.config.cache.list_minutes * 60);
   Ok(fetched)
}

#[page("/{username}/lists/{slug}")]
pub async fn list_by_slug(cx: &Cx) -> topcoat::Result<impl View> {
   let state = state(cx);
   let username = path_param::<Username>(cx);
   let slug = path_param::<Slug>(cx);
   let query = query_params::<ListQuery>(cx)?;
   let prefs = request_prefs(cx);

   let list = state.api.get_list_by_slug(username, slug).await?;
   let tweets = state
      .api
      .get_list_tweets(&list.id, query.cursor.as_deref())
      .await?;
   let groups = maybe_filter_retweets(tweets.content, query.no_retweets.as_deref() == Some("true"));
   let base_url = format!("/{username}/lists/{slug}");
   let title = format!("{} - List by @{username}", list.name);

   Ok(view! {
      document(
         title: &title,
         prefs: &prefs,
         description: &list.description,
         <div class="timeline-container">
            list_header(list: &list, active: ListTab::Tweets)
            timeline(groups: &groups, prefs: &prefs, cursor: tweets.bottom.as_deref(), base_url: &base_url)
         </div>
      )
   })
}

#[page("/i/lists/{id}")]
pub async fn list_by_id(cx: &Cx) -> topcoat::Result<impl View> {
   let state = state(cx);
   let id = path_param::<Id>(cx);
   let query = query_params::<ListQuery>(cx)?;
   let prefs = request_prefs(cx);

   let (list, tweets) = tokio::join!(
      cached_list(state, id),
      state.api.get_list_tweets(id, query.cursor.as_deref()),
   );
   let list = list?;
   let tweets = tweets?;
   let groups = maybe_filter_retweets(tweets.content, query.no_retweets.as_deref() == Some("true"));
   let base_url = format!("/i/lists/{id}");
   let title = format!("{} - List", list.name);

   Ok(view! {
      document(
         title: &title,
         prefs: &prefs,
         description: &list.description,
         <div class="timeline-container">
            list_header(list: &list, active: ListTab::Tweets)
            timeline(groups: &groups, prefs: &prefs, cursor: tweets.bottom.as_deref(), base_url: &base_url)
         </div>
      )
   })
}

async fn cached_members(
   state: &AppState,
   id: &str,
   cursor: Option<&str>,
) -> Result<PaginatedResult<User>> {
   if cursor.is_some() {
      return state.api.get_list_members(id, cursor).await;
   }
   let cache_key = cache_keys::list_members(id);
   if let Some(cached) = state.cache.get::<PaginatedResult<User>>(&cache_key) {
      tracing::debug!("Cache hit for list members: {id}");
      return Ok(cached);
   }
   let members = state.api.get_list_members(id, None).await?;
   state
      .cache
      .set(&cache_key, &members, state.config.cache.list_minutes * 60);
   Ok(members)
}

#[page("/i/lists/{id}/members")]
pub async fn list_members(cx: &Cx) -> topcoat::Result<impl View> {
   let state = state(cx);
   let id = path_param::<Id>(cx);
   let query = query_params::<ListQuery>(cx)?;
   let prefs = request_prefs(cx);

   let (list, members) = tokio::join!(
      cached_list(state, id),
      cached_members(state, id, query.cursor.as_deref()),
   );
   let list = list?;
   let members = members?;
   let base_url = format!("/i/lists/{id}/members");
   let title = format!("{} - Members", list.name);

   Ok(view! {
      document(
         title: &title,
         prefs: &prefs,
         <div class="timeline-container">
            list_header(list: &list, active: ListTab::Members)
            user_list(
               users: &members.content,
               prefs: &prefs,
               cursor: members.bottom.as_deref(),
               base_url: &base_url,
            )
         </div>
      )
   })
}
