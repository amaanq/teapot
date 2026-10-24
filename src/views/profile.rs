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
      Child,
      View,
      class,
      component,
      view,
   },
};

use crate::{
   types::{
      prefs::Prefs,
      timeline::TimelineKind,
      tweet::GalleryPhoto,
      user::User,
   },
   utils::{
      country::{
         display_name as country_display_name,
         flag as country_flag,
      },
      formatters,
   },
   views::{
      renderutils::{
         config,
         get_avatar_class,
         icon,
         link_user,
         parse_location,
         proxied_image,
         render_bio_html,
         short_link,
         trusted,
         verified_icon,
      },
      timeline::timeline_tabs,
   },
};

/// Profile chrome (banner, card, photo rail and tabs) around a timeline.
/// Suspended and protected accounts get a notice in place of the timeline.
#[component]
pub async fn profile_page<'a>(
   cx: &Cx,
   user: &'a User,
   photo_rail: &'a [GalleryPhoto],
   prefs: &'a Prefs,
   tab: TimelineKind,
   #[default] child: Child<'a>,
) -> Result<impl View> {
   let config = config(cx);
   let banner = &user.banner;

   Ok(view! {
      <div class="profile-tabs">
         <div class=(class!("profile-tab", "sticky" if prefs.sticky_profile))>
            <div class="profile-header">
               if !prefs.hide_banner {
                  <div class="profile-banner">
                     if banner.is_empty() {
                        <a></a>
                     } else if banner.starts_with('#') {
                        <a style=(format!("background-color: {banner}"))></a>
                     } else {
                        <a href=(formatters::get_pic_url(banner, config.config.base64_media)) target="_blank">
                           proxied_image(url: banner)
                        </a>
                     }
                  </div>
               }
               user_card(user: user, prefs: prefs)
            </div>
            if !photo_rail.is_empty() {
               photo_rail_card(photos: photo_rail, user: user)
            }
         </div>
         if user.suspended {
            <div class="timeline-container">
               <div class="timeline-header">
                  <h2>"@" (&user.username) " is suspended"</h2>
                  <p>"X has suspended this account, so its posts are not available here."</p>
               </div>
            </div>
         } else if user.protected {
            <div class="timeline-container">
               <div class="timeline-header timeline-protected">
                  <h2>"@" (&user.username) " is protected"</h2>
                  <p>"Only followers this account has approved can see its posts."</p>
               </div>
            </div>
         } else {
            <div class="timeline-container">
               timeline_tabs(active: tab, username: &user.username)
               (child)
            </div>
         }
      </div>
   })
}

const PROFILE_STATS: [(&str, &str); 4] = [
   ("posts", "Tweets"),
   ("following", "Following"),
   ("followers", "Followers"),
   ("likes", "Likes"),
];

/// Abbreviate a stat so the sidebar columns stay aligned. The exact figure
/// stays reachable through the element's title.
fn compact_profile_stat(num: i64) -> String {
   #[expect(
      clippy::cast_precision_loss,
      reason = "display only, and the abbreviation rounds anyway"
   )]
   let value = num.unsigned_abs() as f64;
   let (scaled, suffix) = if value >= 1_000_000_000.0 {
      (value / 1_000_000_000.0, "B")
   } else if value >= 1_000_000.0 {
      (value / 1_000_000.0, "M")
   } else if value >= 1_000.0 {
      (value / 1_000.0, "K")
   } else {
      return num.to_string();
   };
   let precision = usize::from(scaled < 100.0);
   let compact = format!("{scaled:.precision$}")
      .trim_end_matches('0')
      .trim_end_matches('.')
      .to_owned();
   format!("{}{compact}{suffix}", if num < 0 { "-" } else { "" })
}

/// X's "About this account" panel, collapsed behind the country it reports.
#[component]
async fn account_context(user: &User) -> Result<impl View> {
   let country = country_display_name(&user.account_based_in);
   let maybe_proxied = user.location_accurate == Some(false);

   Ok(view! {
      if !user.account_based_in.is_empty() || !user.connection_source.is_empty() {
         <details class="account-context-trigger">
            <summary class="account-signal" title="Show X-reported account information">
               if let Some(flag) = country_flag(country) {
                  <span class="account-signal-flag" role="img" aria-label=(country)>(flag.to_string())</span>
               } else {
                  <span
                     class="icon-location"
                     role="img"
                     aria-label=(if country.is_empty() { "Account information" } else { country })
                  ></span>
               }
               <span class="sr-only">"Account based in"</span>
               if maybe_proxied {
                  <span
                     class="account-signal-warning"
                     title="X says this location may be affected by a proxy or VPN"
                  >
                     "!"
                  </span>
               }
            </summary>
            <section class="account-context" aria-label="About this account">
               <div class="account-context-header">
                  <div><h3>"About this account"</h3></div>
                  <span class="account-context-mark" aria-hidden="true">"i"</span>
               </div>
               <dl>
                  if !user.account_based_in.is_empty() {
                     <div class="account-context-row">
                        <dt>"Account based in"</dt>
                        <dd>(country)</dd>
                     </div>
                  }
                  if !user.connection_source.is_empty() {
                     <div class="account-context-row">
                        <dt>"Connected via"</dt>
                        <dd>(&user.connection_source)</dd>
                     </div>
                  }
               </dl>
               if maybe_proxied {
                  <p class="account-context-warning">
                     <span aria-hidden="true">"!"</span>
                     " X says this location may be affected by a proxy or VPN."
                  </p>
               }
               <p class="account-context-note">
                  "Account-level information from X, not a live location or the device used "
                  "for a specific post."
               </p>
            </section>
         </details>
      }
   })
}

/// Render user card.
#[component]
async fn user_card(cx: &Cx, user: &User, prefs: &Prefs) -> Result<impl View> {
   let config = config(cx);

   // Use _400x400 suffix for avatar, unless it's a gif and autoplay is on.
   // get_user_pic strips any existing size suffix (_normal, _400x400, etc.)
   // and inserts the new one.
   let avatar_pic = if user.user_pic.is_empty() {
      String::new()
   } else if prefs.autoplay_gifs && user.user_pic.ends_with("gif") {
      formatters::get_user_pic(&user.user_pic, "")
   } else {
      formatters::get_user_pic(&user.user_pic, "_400x400")
   };
   let (place, place_url) = parse_location(&user.location);
   let short_place = if place.chars().count() > 40 {
      format!("{}…", place.chars().take(40).collect::<String>())
   } else {
      place.clone()
   };
   let place_href = if !place_url.is_empty() {
      Some(place_url)
   } else if place.contains("://") {
      Some(place.clone())
   } else {
      None
   };
   let website = formatters::replace_urls(&user.website, config);
   let join_date = user.join_date.map(|date| {
      (
         date.strftime("%-I:%M %p - %-d %b %Y").to_string(),
         formatters::format_join_date(date),
      )
   });
   let stats = [user.tweets, user.following, user.followers, user.likes];

   Ok(view! {
      <div class="profile-card">
         <a
            class="profile-card-avatar"
            href=(formatters::get_pic_url(&user.user_pic, config.config.base64_media))
            target="_blank"
         >
            proxied_image(url: &avatar_pic, class: get_avatar_class(prefs))
         </a>
         <div class="profile-card-identity">
            <div>
               link_user(user: user, class: "profile-card-fullname")
               " "
               verified_icon(user: user)
            </div>
            link_user(user: user, class: "profile-card-username")
            account_context(user: user)
         </div>
         <div class="profile-card-extra">
            if !user.bio.is_empty() {
               <div class="profile-bio">
                  <p dir="auto">(trusted(render_bio_html(&user.bio, config)))</p>
               </div>
            }
            if !user.location.is_empty() {
               <div class="profile-location">
                  <span>icon(name: "location")</span>
                  " "
                  if let Some(href) = place_href {
                     <a href=(href) title=(&place)>(short_place)</a>
                  } else {
                     <span title=(&place)>(short_place)</span>
                  }
               </div>
            }
            if !user.website.is_empty() {
               <div class="profile-website">
                  <span>
                     icon(name: "link")
                     " "
                     <a rel="me" href=(&website)>(short_link(&website))</a>
                  </span>
               </div>
            }
            if let Some((full_date, short_date)) = join_date {
               <div class="profile-joindate">
                  <span title=(full_date)>icon(name: "calendar", text: &short_date)</span>
               </div>
            }
            <div class="profile-card-extra-links">
               <ul class="profile-statlist">
                  for ((class, label), num) in PROFILE_STATS.into_iter().zip(stats) {
                     <li class=(class)>
                        <span class="profile-stat-header">(label)</span>
                        <span class="profile-stat-num" title=(formatters::format_with_commas(num))>
                           (compact_profile_stat(num))
                        </span>
                     </li>
                  }
               </ul>
            </div>
         </div>
      </div>
   })
}

/// Render photo rail showing recent media.
#[component]
async fn photo_rail_card(photos: &[GalleryPhoto], user: &User) -> Result<impl View> {
   let count = compact_profile_stat(user.media);
   let exact_count = formatters::format_with_commas(user.media);
   let thumbs = photos.iter().take(15).map(|photo| {
      let suffix = if photo.url.contains("format") || photo.url.contains("placeholder") {
         ""
      } else {
         ":thumb"
      };
      (&photo.tweet_id, format!("{}{suffix}", photo.url))
   });

   Ok(view! {
      <div class="photo-rail-card">
         <div class="photo-rail-header">
            <a href=(format!("/{}/media", user.username))>
               <span class="photo-rail-icon-wrap">
                  <span class="icon-picture photo-rail-media-icon" aria-hidden="true"></span>
                  <span class="photo-rail-media-badge" title=(&exact_count)>(&count)</span>
               </span>
               <span class="photo-rail-media-label">"Media"</span>
            </a>
         </div>
         <input id="photo-rail-grid-toggle" type="checkbox">
         <label for="photo-rail-grid-toggle" class="photo-rail-header-mobile">
            <div class="photo-rail-header-mobile-inner">
               <span class="photo-rail-icon-wrap">
                  <span class="icon-picture photo-rail-media-icon" aria-hidden="true"></span>
                  <span class="photo-rail-media-badge" title=(&exact_count)>(&count)</span>
               </span>
               <span>"Media"</span>
            </div>
            icon(name: "down")
         </label>
         <div class="photo-rail-grid">
            for (tweet_id, url) in thumbs {
               <a href=(format!("/{}/status/{tweet_id}#m", user.username))>
                  proxied_image(url: &url)
               </a>
            }
         </div>
      </div>
   })
}
