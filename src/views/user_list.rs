#![expect(
   clippy::same_name_method,
   clippy::shadow_same,
   clippy::shadow_unrelated,
   reason = "topcoat's #[component] and view! expansions"
)]

use topcoat::{
   Result,
   context::Cx,
   view::{
      View,
      component,
      view,
   },
};

use crate::{
   types::{
      prefs::Prefs,
      user::User,
   },
   utils::formatters,
   views::renderutils::{
      config,
      get_avatar_class,
      link_user,
      render_bio_html,
      trusted,
      verified_icon,
   },
};

/// Render a list of users in timeline format.
#[component]
pub async fn user_list<'a>(
   users: &'a [User],
   prefs: &'a Prefs,
   #[default] cursor: Option<&'a str>,
   #[default] base_url: &'a str,
) -> Result<impl View> {
   let load_more_url = cursor.map(|cursor| formatters::cursor_url(base_url, cursor));
   Ok(view! {
      <div class="timeline">
         if users.is_empty() {
            <div class="timeline-none"><h2 class="timeline-none">"No users found"</h2></div>
         } else {
            for user in users {
               user_item(user: user, prefs: prefs)
            }
            if let Some(url) = load_more_url {
               <div class="show-more"><a href=(url)>"Load more"</a></div>
            }
         }
      </div>
   })
}

/// Render a single user in timeline format.
#[component]
pub async fn user_item(cx: &Cx, user: &User, prefs: &Prefs) -> Result<impl View> {
   let config = config(cx);
   let href = format!("/{}", user.username);
   Ok(view! {
      <div class="timeline-item" data-username=(&user.username)>
         <a class="tweet-link" href=(&href)></a>
         <div class="tweet-body profile-result">
            <div class="tweet-header">
               <a class="tweet-avatar" href=(&href)>
                  <img
                     src=(formatters::get_pic_url(&user.user_pic, config.config.base64_media))
                     class=(get_avatar_class(prefs))
                     alt=""
                     loading="lazy"
                  >
               </a>
               <div class="tweet-name-row">
                  <div class="fullname-and-username">
                     link_user(user: user, class: "fullname")
                     verified_icon(user: user)
                  </div>
               </div>
               link_user(user: user, class: "username")
            </div>
            if !user.bio.is_empty() {
               <div class="tweet-content media-body" dir="auto">
                  (trusted(render_bio_html(&user.bio, config)))
               </div>
            }
         </div>
      </div>
   })
}
