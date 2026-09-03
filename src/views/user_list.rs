use maud::{
   Markup,
   PreEscaped,
   html,
};

use crate::{
   config::Config,
   types::{
      prefs::Prefs,
      user::User,
   },
   utils::formatters,
   views::renderutils::{
      get_avatar_class,
      link_user,
      render_bio_html,
      verified_icon,
   },
};

/// Render a list of users in timeline format.
#[expect(
   clippy::module_name_repetitions,
   reason = "render_user_list is the canonical name"
)]
pub fn render_user_list(
   users: &[User],
   config: &Config,
   cursor: Option<&str>,
   base_url: Option<&str>,
   prefs: &Prefs,
) -> Markup {
   let load_more_url = match (cursor, base_url) {
      (Some(cur), Some(base)) => Some(formatters::cursor_url(base, cur)),
      (Some(cur), None) => Some(formatters::cursor_url("", cur)),
      _ => None,
   };

   html! {
       div class="timeline" {
           @if users.is_empty() {
               div class="timeline-none" {
                   h2 class="timeline-none" { "No users found" }
               }
           } @else {
               @for user in users {
                   (render_user(user, config, prefs))
               }
               @if let Some(ref url) = load_more_url {
                   div class="show-more" {
                       a href=(url) {
                           "Load more"
                       }
                   }
               }
           }
       }
   }
}

/// Render a single user in timeline format.
///
/// Used by both `user_list` and `search` views.
pub fn render_user(user: &User, config: &Config, prefs: &Prefs) -> Markup {
   let href = format!("/{}", user.username);
   let avatar_url = formatters::get_pic_url(&user.user_pic, config.config.base64_media);
   let avatar_class = get_avatar_class(prefs);

   html! {
       div class="timeline-item" data-username=(user.username) {
           a class="tweet-link" href=(&href) {}
           div class="tweet-body profile-result" {
               div class="tweet-header" {
                   a class="tweet-avatar" href=(&href) {
                       img src=(avatar_url) class=(avatar_class) alt="" loading="lazy";
                   }

                   div class="tweet-name-row" {
                       div class="fullname-and-username" {
                           (link_user(user, "fullname"))
                           (verified_icon(user))
                       }
                   }
                   (link_user(user, "username"))
               }

               @if !user.bio.is_empty() {
                   div class="tweet-content media-body" dir="auto" {
                       (PreEscaped(&render_bio_html(&user.bio, config)))
                   }
               }
           }
       }
   }
}
