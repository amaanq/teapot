//! The route table.

use std::sync::Arc;

use axum::Router;
use topcoat::{
   asset::{
      AssetBundle,
      RouterBuilderAssetExt as _,
   },
   cookie::RouterBuilderCookieExt as _,
   router::{
      Page as _,
      Router as PageRouter,
      tower::TowerService,
   },
   runtime::RouterBuilderRuntimeExt as _,
};

use crate::{
   AppState,
   routes::{
      debug,
      embed,
      feed,
      helpers,
      intent,
      list,
      media,
      notes,
      pages,
      preferences,
      redirect,
      rss,
      search,
      status,
      timeline,
      unsupported,
   },
   views::tweet,
};

/// Pages render through topcoat, and axum forwards each page's path to it so
/// the axum middleware still sees the matched path parameters.
pub fn router(state: &AppState, assets: AssetBundle) -> Router<AppState> {
   let mut paths = Vec::new();
   let mut builder = PageRouter::builder()
      .app_context(state.clone())
      .app_context(Arc::clone(&state.config))
      .assets(assets)
      .cookies()
      .layout(helpers::error_layout)
      .route(preferences::save_toggle)
      .route(preferences::save_text)
      .route(tweet::translate)
      .route(feed::more)
      .runtime();

   macro_rules! pages {
      ($($page:expr),* $(,)?) => {
         $(
            paths.push($page.path().to_matchit_path());
            builder = builder.page($page);
         )*
      };
   }

   pages![
      pages::home,
      pages::about,
      preferences::settings,
      search::search,
      search::hashtag,
      intent::intent_user,
      intent::intent_follow,
      unsupported::intent_unsupported,
      unsupported::login,
      unsupported::login_subpath,
      unsupported::about_feature,
      unsupported::i_unsupported,
      embed::tweet_embed_page,
      embed::video_embed_page,
      status::status,
      status::statuses,
      status::status_photo,
      status::status_by_id,
      status::web_status_by_id,
      status::user_by_id,
      status::retweets,
      status::quotes,
      status::edit_history,
      list::user_lists_unsupported,
      list::list_by_slug,
      list::list_by_id,
      list::list_members,
      notes::show_note,
      notes::show_note_by_id,
      timeline::user_timeline,
      timeline::user_replies,
      timeline::user_media,
      timeline::user_search,
   ];

   let service = TowerService::new(builder.build());
   let router = Router::new()
      .merge(pages::router())
      .merge(debug::router())
      .merge(redirect::router())
      .merge(search::router())
      .merge(preferences::router())
      .merge(embed::router())
      .merge(status::router())
      .merge(media::router())
      .merge(rss::router())
      .route_service("/_topcoat/{*rest}", service.clone());
   paths.iter().fold(router, |router, path| {
      router.route_service(path, service.clone())
   })
}
