use maud::{
   Markup,
   PreEscaped,
   html,
};

use super::timeline::{
   render_timeline_tabs,
   render_timeline_with_pinned_and_prefs,
   tab_to_kind,
};
use crate::{
   config::Config,
   types::{
      GalleryPhoto,
      Prefs,
      Profile,
      TimelineKind,
      User,
   },
   utils::formatters,
   views::renderutils::{
      gen_img,
      get_avatar_class,
      icon,
      link_user,
      parse_location,
      render_bio_html,
      short_link,
      verified_icon,
   },
};

/// Render a user profile page with prefs support (for bidi).
pub fn render_profile_with_prefs(
   profile: &Profile,
   config: &Config,
   tab: &str,
   prefs: Option<&Prefs>,
   newer_url: Option<&str>,
) -> Markup {
   let groups = &profile.tweets.content;
   let cursor = profile.tweets.bottom.as_deref();
   let base_url = format!("/{}", profile.user.username);
   let timeline_kind = tab_to_kind(tab);

   // Only show pinned tweet on the main Tweets tab
   let pinned = if tab.is_empty() || tab == "tweets" {
      profile.pinned.as_ref()
   } else {
      None
   };

   let hide_banner = prefs.is_some_and(|pref| pref.hide_banner);
   let sticky = prefs.is_some_and(|pref| pref.sticky_profile);
   let profile_tab_class = if sticky {
      "profile-tab sticky"
   } else {
      "profile-tab"
   };

   html! {
       div class="profile-tabs" {
           // Left sidebar with profile card and photo rail
           div class=(profile_tab_class) {
               div class="profile-header" {
                   @if !hide_banner {
                       div class="profile-banner" {
                           (render_banner(&profile.user.banner, config))
                       }
                   }
                   (render_user_card(&profile.user, config, prefs))
               }

               // Photo rail positioned on the left side
               @if !profile.photo_rail.is_empty() {
                   (render_photo_rail(&profile.photo_rail, &profile.user, config))
               }
           }

           // Check for suspended or protected account
           @if profile.user.suspended {
               div class="timeline-container" {
                   div class="timeline-header" {
                       h2 { "Account suspended" }
                       p { "Twitter suspends accounts that violate the Twitter Rules." }
                   }
               }
           } @else if profile.user.protected {
               div class="timeline-container" {
                   div class="timeline-header timeline-protected" {
                       h2 { "This account's tweets are protected." }
                       p { "Only confirmed followers have access to @" (profile.user.username) "'s tweets." }
                   }
               }
           } @else {
               // Timeline container - main content area
               div class="timeline-container" {
                   (render_timeline_tabs(timeline_kind, &profile.user.username))
                   (render_timeline_with_pinned_and_prefs(groups, config, cursor, Some(&base_url), pinned, prefs, newer_url))
               }
           }
       }
   }
}

/// Render the full profile wrapper (banner + sidebar + photo rail) with custom
/// timeline content. Used by sub-tab routes (`with_replies`, media, search)
/// that fetch their own timeline data but need the same profile chrome as the
/// main Tweets tab.
pub fn render_profile_page(
   user: &User,
   photo_rail: &[GalleryPhoto],
   config: &Config,
   prefs: &Prefs,
   tab: TimelineKind,
   timeline_content: &Markup,
) -> Markup {
   let hide_banner = prefs.hide_banner;
   let sticky = prefs.sticky_profile;
   let profile_tab_class = if sticky {
      "profile-tab sticky"
   } else {
      "profile-tab"
   };

   html! {
       div class="profile-tabs" {
           // Left sidebar
           div class=(profile_tab_class) {
               div class="profile-header" {
                   @if !hide_banner {
                       div class="profile-banner" {
                           (render_banner(&user.banner, config))
                       }
                   }
                   (render_user_card(user, config, Some(prefs)))
               }

               // Photo rail on the left sidebar
               @if !photo_rail.is_empty() {
                   (render_photo_rail(photo_rail, user, config))
               }
           }

           // Main content area
           div class="timeline-container" {
               (render_timeline_tabs(tab, &user.username))
               (timeline_content)
           }
       }
   }
}

/// Render a single profile stat.
fn render_stat(num: i64, class: &str, text: &str) -> Markup {
   let display_text = if text.is_empty() {
      // Capitalize class name
      let mut chars = class.chars();
      chars.next().map_or_else(String::new, |ch| {
         format!("{}{}", ch.to_uppercase(), chars.as_str())
      })
   } else {
      text.to_owned()
   };

   html! {
       li class=(class) {
           span class="profile-stat-header" { (display_text) }
           span class="profile-stat-num" { (formatters::format_with_commas(num)) }
       }
   }
}

/// Render user card (profile sidebar).
fn render_user_card(user: &User, config: &Config, prefs: Option<&Prefs>) -> Markup {
   let avatar_class = get_avatar_class(prefs);

   // Use _400x400 suffix for avatar, unless it's a gif and autoplay is on.
   // get_user_pic strips any existing size suffix (_normal, _400x400, etc.)
   // and inserts the new one.
   let autoplay_gifs = prefs.is_none_or(|pref| pref.autoplay_gifs);
   let avatar_pic = if user.user_pic.is_empty() {
      String::new()
   } else if autoplay_gifs && user.user_pic.ends_with("gif") {
      formatters::get_user_pic(&user.user_pic, "")
   } else {
      formatters::get_user_pic(&user.user_pic, "_400x400")
   };

   let orig_avatar_url = formatters::get_pic_url(&user.user_pic, config.config.base64_media);

   let bio_html = render_bio_html(&user.bio, config);

   html! {
       div class="profile-card" {
           a class="profile-card-avatar" href=(orig_avatar_url) target="_blank" {
               (gen_img(&avatar_pic, avatar_class, config))
           }

           div class="profile-card-identity" {
               div {
                   (link_user(user, "profile-card-fullname"))
                   " "
                   (verified_icon(user))
               }
               (link_user(user, "profile-card-username"))
           }

           div class="profile-card-extra" {
               @if !user.bio.is_empty() {
                   div class="profile-bio" {
                       p dir="auto" { (PreEscaped(&bio_html)) }
                   }
               }

               @if !user.location.is_empty() {
                   div class="profile-location" {
                       span { (icon("location", "", "", "", "")) }
                       " "
                       @let (place, url) = parse_location(&user.location);
                       @let display_place = if place.chars().count() > 40 {
                           format!("{}…", place.chars().take(40).collect::<String>())
                       } else {
                           place.clone()
                       };
                       @if !url.is_empty() {
                           a href=(url) title=(place) { (display_place) }
                       } @else if place.contains("://") {
                           a href=(place) title=(place) { (display_place) }
                       } @else {
                           span title=(place) { (display_place) }
                       }
                   }
               }

               @if !user.website.is_empty() {
                   div class="profile-website" {
                       span {
                           @let url = formatters::replace_urls(&user.website, config);
                           (icon("link", "", "", "", ""))
                           " "
                           a rel="me" href=(url) { (short_link(&url)) }
                       }
                   }
               }

               @if let Some(join_date) = user.join_date {
                   @let full_date = {
                       let hour_24 = join_date.hour();
                       let (hour_12, period) = match hour_24 {
                           0 => (12_u8, "AM"),
                           1..=11 => (hour_24, "AM"),
                           12 => (12, "PM"),
                           _ => (hour_24 - 12, "PM"),
                       };
                       let date_part = join_date.format(time::macros::format_description!("[day padding:none] [month repr:short] [year]")).unwrap_or_default();
                       format!("{hour_12}:{:02} {period} - {date_part}", join_date.minute())
                   };
                   @let short_date = formatters::format_join_date(join_date);
                   div class="profile-joindate" {
                       span title=(full_date) {
                           (icon("calendar", &short_date, "", "", ""))
                       }
                   }
               }

               div class="profile-card-extra-links" {
                   ul class="profile-statlist" {
                       (render_stat(user.tweets, "posts", "Tweets"))
                       (render_stat(user.following, "following", ""))
                       (render_stat(user.followers, "followers", ""))
                       (render_stat(user.likes, "likes", ""))
                   }
               }
           }
       }
   }
}

/// Render banner.
fn render_banner(banner: &str, config: &Config) -> Markup {
   if banner.is_empty() {
      html! { a {} }
   } else if banner.starts_with('#') {
      html! { a style=(format!("background-color: {banner}")) {} }
   } else {
      html! {
          a href=(formatters::get_pic_url(banner, config.config.base64_media)) target="_blank" {
              (gen_img(banner, "", config))
          }
      }
   }
}

/// Render photo rail showing recent media.
fn render_photo_rail(photos: &[GalleryPhoto], user: &User, config: &Config) -> Markup {
   let count = formatters::format_with_commas(user.media);

   html! {
       div class="photo-rail-card" {
           div class="photo-rail-header" {
               a href=(format!("/{}/media", user.username)) {
                   span class="photo-rail-icon-wrap" {
                       span class="material-symbols-outlined photo-rail-media-icon" { "photo_library" }
                       span class="photo-rail-media-badge" { (count) }
                   }
                   span class="photo-rail-media-label" { "Media" }
               }
           }

           // Mobile toggle
           input id="photo-rail-grid-toggle" type="checkbox";
           label for="photo-rail-grid-toggle" class="photo-rail-header-mobile" {
               div class="photo-rail-header-mobile-inner" {
                   span class="photo-rail-icon-wrap" {
                       span class="material-symbols-outlined photo-rail-media-icon" { "photo_library" }
                       span class="photo-rail-media-badge" { (count) }
                   }
                   span { "Media" }
               }
               (icon("down", "", "", "", ""))
           }

           div class="photo-rail-grid" {
               @for photo in photos.iter().take(10) {
                   @let photo_suffix = if photo.url.contains("format") || photo.url.contains("placeholder") { "" } else { ":thumb" };
                   a href=(format!("/{}/status/{}#m", user.username, photo.tweet_id)) {
                       (gen_img(&format!("{}{}", photo.url, photo_suffix), "", config))
                   }
               }
           }
       }
   }
}
