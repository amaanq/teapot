use super::*;
use crate::{
   api::schema::CommunityNote,
   config::{
      AppConfig,
      CacheConfig,
      GifTranscodingConfig,
      PreferencesConfig,
      ServerConfig,
   },
   types::{
      Card,
      Photo,
      Poll,
      User,
      VideoType,
      VideoVariant,
   },
};

fn test_config() -> Config {
   Config {
      server:          ServerConfig {
         hostname:             "teapot.test".to_owned(),
         title:                "teapot".to_owned(),
         address:              "127.0.0.1".to_owned(),
         port:                 443,
         public_port:          None,
         https:                true,
         http_max_connections: 100,
         static_dir:           "./public".to_owned(),
      },
      cache:           CacheConfig {
         list_minutes: 120,
         rss_minutes:  10,
         max_entries:  50_000,
      },
      config:          AppConfig {
         hmac_key:            "0123456789abcdef0123456789abcdef".to_owned(),
         base64_media:        true,
         enable_rss:          true,
         enable_debug:        false,
         debug_token:         String::new(),
         proxy:               String::new(),
         proxy_auth:          String::new(),
         api_proxy:           String::new(),
         disable_tid:         false,
         max_concurrent_reqs: 2,
         paid_emoji:          ":paid:".to_owned(),
         ai_emoji:            ":ai:".to_owned(),
         kagi_token:          String::new(),
         kagi_token_file:     String::new(),
      },
      preferences:     PreferencesConfig::default(),
      gif_transcoding: GifTranscodingConfig::default(),
      url_prefix:      "https://teapot.test".to_owned(),
   }
}

fn user(username: &str) -> User {
   User {
      id: username.to_owned(),
      username: username.to_owned(),
      fullname: username.to_owned(),
      user_pic: "https://pbs.twimg.com/profile_images/avatar.jpg".to_owned(),
      ..User::default()
   }
}

fn tweet(id: i64, username: &str, text: &str) -> Tweet {
   Tweet {
      id,
      user: user(username),
      text: text.to_owned(),
      available: true,
      ..Tweet::default()
   }
}

fn video() -> Video {
   Video {
      thumb: "https://pbs.twimg.com/ext_tw_video_thumb/thumb.jpg".to_owned(),
      variants: vec![VideoVariant {
         content_type: VideoType::Mp4,
         url:          "https://video.twimg.com/ext_tw_video/1/pu/vid/720x1280/video.mp4?tag=12"
            .to_owned(),
         bitrate:      2_176_000,
         resolution:   1280,
      }],
      ..Video::default()
   }
}

fn quoted_video_tweet() -> Tweet {
   let mut outer = tweet(100, "outer", "outer text");
   let mut quote = tweet(200, "quote", "quote text");
   quote.video = Some(video());
   outer.quote = Some(Box::new(quote));
   outer
}

#[test]
fn quoted_video_player_uses_quote_tweet_id() {
   let html = render_tweet_embed(&quoted_video_tweet(), &test_config(), false).into_string();

   assert!(
      html.contains(r#"name="twitter:player" content="https://teapot.test/i/videos/tweet/200""#)
   );
   assert!(
      !html.contains(r#"name="twitter:player" content="https://teapot.test/i/videos/tweet/100""#)
   );
   assert!(html.contains(r#"name="twitter:player:stream" content="https://teapot.test/video/"#));
   assert!(html.contains("/enc/"));
}

#[test]
fn video_embed_uses_signed_proxy_for_quoted_video() {
   let tweet = quoted_video_tweet();
   let html = render_video_embed(&tweet, &test_config()).into_string();

   assert!(has_playable_video(&tweet));
   assert!(html.contains(r#"poster="/pic/enc/"#));
   assert!(html.contains(r#"<source src="/video/"#));
   assert!(html.contains(r#"name="twitter:player:stream" content="https://teapot.test/video/"#));
   assert!(html.contains("/enc/"));
   assert!(!html.contains("https://teapot.test/video/https://video.twimg.com"));
}

#[test]
fn activity_pub_quote_fallback_uses_signed_media_urls() {
   let mut tweet = quoted_video_tweet();
   tweet.reply = vec!["parent".to_owned()];
   tweet.reply_mentions_stripped = true;

   let activity = build_activity_pub(&tweet, &test_config());
   let attachment = activity.media_attachments.first().unwrap();

   assert_eq!(activity.media_attachments.len(), 1);
   assert_eq!(attachment.type_, "video");
   assert!(attachment.url.starts_with("https://teapot.test/video/"));
   assert!(attachment.url.contains("/enc/"));
   assert!(!attachment.url.contains("/video/https://video.twimg.com"));
   assert!(
      attachment
         .preview_url
         .as_ref()
         .is_some_and(|url| url.starts_with("https://teapot.test/pic/enc/"))
   );
   let dimensions = &attachment.meta.as_ref().unwrap().original;
   assert_eq!((dimensions.width, dimensions.height), (720, 1280));
   assert_eq!(dimensions.size, "720x1280");
   assert!(
      activity
         .content
         .contains(r#"<sub>↩ <a href="https://x.com/parent" class="u-url mention"> (@parent)</a>"#)
   );
   assert!(
      activity
         .content
         .contains(r#"</sub><br><a href="https://x.com/parent">@parent</a> outer text"#)
   );
}

#[test]
fn activity_reply_context_links_and_truncates_original_post() {
   let mut reply = tweet(402, "replier", "reply text");
   reply.reply_id = 401;
   reply.reply = vec!["parent".to_owned()];
   reply.reply_mentions_stripped = true;
   let original_text = format!("{} final marker", "original context ".repeat(20));
   let original = tweet(401, "parent", &original_text);

   let activity = build_activity_pub_with_reply(&reply, Some(&original), &test_config());

   let context_start = activity
         .content
         .find(r#"<blockquote><b>↩ <a href="https://x.com/parent/status/401">Original post</a> · <a href="https://x.com/parent">@parent</a></b><br>"#)
         .unwrap();
   assert!(activity.content.find("reply text").unwrap() < context_start);
   assert!(!activity.content.contains("<sub>↩"));
   assert!(!activity.content.contains(">@parent</a> reply text"));
   assert!(activity.content.contains("original context"));
   assert!(activity.content.contains('…'));
   assert!(!activity.content.contains("final marker"));
}

#[test]
fn activity_payload_preserves_intentional_space_runs() {
   let status = tweet(403, "spaced", "How it started     How it's going");

   let activity = build_activity_pub(&status, &test_config());

   assert!(
      activity
         .content
         .contains("How it started&nbsp; &nbsp; &nbsp;How it's going")
   );
}

#[test]
fn activity_payload_includes_community_note_with_sources() {
   let mut status = tweet(403, "noted", "A claim");
   let note_text = "Germany did so in 2002. Source";
   let source_start = note_text.find("Source").unwrap();
   status.note = Some(CommunityNote {
      text:  note_text.to_owned(),
      links: vec![(
         source_start,
         source_start + "Source".chars().count(),
         "https://example.com/source".to_owned(),
      )],
   });

   let activity = build_activity_pub(&status, &test_config());

   assert!(activity.content.contains(
         r#"<blockquote><b>📝 Community Note</b><br>Germany did so in 2002. <a href="https://example.com/source">Source</a></blockquote>"#,
      ));
}

#[test]
fn activity_payload_marks_sensitive_media() {
   let mut status = tweet(403, "sensitive", "content warning");
   status.sensitive = true;
   status.photos.push(Photo {
      url: "https://pbs.twimg.com/media/sensitive.jpg".to_owned(),
      width: 1200,
      height: 800,
      ..Photo::default()
   });

   let activity = build_activity_pub(&status, &test_config());

   assert!(activity.sensitive);
   assert!(activity.spoiler_text.is_empty());
   assert_eq!(activity.media_attachments.len(), 1);
}

#[test]
fn activity_payload_renders_article_title_and_excerpt() {
   let mut status = tweet(404, "writer", "");
   status.card = Some(Card {
      kind: CardKind::Article,
      url: "/writer/article/404".to_owned(),
      title: "Defining Taste".to_owned(),
      text: "Taste is the ability to make qualitative judgments.".to_owned(),
      dest: "Article · @writer".to_owned(),
      ..Card::default()
   });

   let activity = build_activity_pub(&status, &test_config());

   assert!(activity.content.contains(
         r#"<blockquote><b>📄 <a href="https://teapot.test/writer/article/404">Defining Taste</a></b><br>Taste is the ability to make qualitative judgments.</blockquote>"#,
      ));
   assert!(!activity.content.starts_with("<br>"));
}

#[test]
fn activity_payload_renders_audio_space_preview() {
   let mut status = tweet(405, "host", "Join our Space");
   status.card = Some(Card {
      kind: CardKind::Audiospace,
      url: "https://x.com/i/spaces/1AxRnnrNvyDxl".to_owned(),
      title: "Building in public".to_owned(),
      text: "Scheduled · Jul 16, 2026 · 8:00 AM UTC".to_owned(),
      dest: "Hosted by Example (@host)".to_owned(),
      ..Card::default()
   });

   let activity = build_activity_pub(&status, &test_config());

   assert!(activity.content.contains(
         r#"<blockquote><b>🎙️ <a href="https://x.com/i/spaces/1AxRnnrNvyDxl">Building in public</a></b><br>Scheduled · Jul 16, 2026 · 8:00 AM UTC<br>Hosted by Example (@host)</blockquote>"#,
      ));
}

#[test]
fn activity_payload_renders_live_broadcast_preview() {
   let mut status = tweet(406, "tbpn", "Going live");
   status.card = Some(Card {
      kind: CardKind::Broadcast,
      url: "https://x.com/i/broadcasts/1XxyggAaLzvGM".to_owned(),
      title: "Stripe x PayPal".to_owned(),
      text: "Live now · 26 watching".to_owned(),
      dest: "Hosted by TBPN (@tbpn)".to_owned(),
      image: "https://video.pscp.tv/latest.jpg".to_owned(),
      ..Card::default()
   });

   let activity = build_activity_pub(&status, &test_config());
   let html = render_status_page(
      &status,
      &html! {},
      &Prefs::default(),
      &test_config(),
      "tbpn",
      "406",
      true,
   )
   .into_string();
   let image_url = format!(
      "https://teapot.test{}",
      formatters::get_pic_url("https://video.pscp.tv/latest.jpg", true)
   );

   assert!(activity.content.contains("📺"));
   assert!(activity.content.contains("Stripe x PayPal"));
   assert!(activity.content.contains("Live now · 26 watching"));
   assert!(activity.content.contains("Hosted by TBPN (@tbpn)"));
   assert_eq!(activity.media_attachments.len(), 1);
   assert_eq!(
      activity.media_attachments[0]
         .meta
         .as_ref()
         .unwrap()
         .original
         .size,
      "1920x1080"
   );
   assert!(html.contains(r#"property="og:site_name" content="teapot""#));
   assert!(html.contains(&format!(r#"property="og:image" content="{image_url}""#)));
   assert!(html.contains(r#"property="og:image:width" content="1920""#));
   assert!(html.contains(r#"property="twitter:card" content="summary_large_image""#));
   assert!(!build_oembed_url(&status, &test_config()).contains("&provider="));

   status.text = "Stripe x PayPal https://t.co/broadcast".to_owned();
   assert_eq!(
      build_discord_embed_description(&status),
      "Stripe x PayPal https://t.co/broadcast"
   );
   let duplicate_activity = build_activity_pub(&status, &test_config());
   assert_eq!(
      duplicate_activity
         .content
         .matches("Stripe x PayPal")
         .count(),
      1
   );
   assert!(
      duplicate_activity
         .content
         .contains("<blockquote>📺 Live now · 26 watching<br>Hosted by TBPN (@tbpn)</blockquote>")
   );

   status.text = "Stripe &amp; PayPal https://t.co/broadcast".to_owned();
   status.card.as_mut().unwrap().title = "Stripe & PayPal".to_owned();
   let entity_activity = build_activity_pub(&status, &test_config());
   assert_eq!(
      entity_activity
         .content
         .matches("Stripe &amp; PayPal")
         .count(),
      1
   );
}

#[test]
fn oembed_footer_renders_content_disclosures() {
   let mut status = tweet(405, "creator", "Generated campaign artwork");
   status.paid_promotion = true;
   status.ai_generated = true;

   let oembed_url = build_oembed_url(&status, &test_config());
   let html = render_tweet_embed(&status, &test_config(), true).into_string();
   let activity = build_activity_pub(&status, &test_config());

   assert!(oembed_url.contains(&format!(
      "&provider={}",
      formatters::url_encode("teapot · :paid: Paid partnership · :ai: Made with AI")
   )));
   assert!(html.contains(
      r#"property="og:site_name" content="teapot · :paid: Paid partnership · :ai: Made with AI""#
   ));
   assert!(!activity.content.contains("Paid partnership"));
   assert!(!activity.content.contains("Made with AI"));
}

#[test]
fn activity_payload_includes_external_link_preview_card() {
   let mut status = tweet(403, "github", "Download the app");
   status.card = Some(Card {
      kind: CardKind::SummaryLarge,
      url: "https://github.com/mobile?utm_source=x".to_owned(),
      title: "GitHub Mobile".to_owned(),
      dest: "github.com".to_owned(),
      text: "The world's development platform, in your pocket".to_owned(),
      image: "https://pbs.twimg.com/card_img/2075083153316749312/qlKLI1ak?format=jpg&name=800x419"
         .to_owned(),
      ..Card::default()
   });

   let activity = build_activity_pub(&status, &test_config());
   let card = activity.card.unwrap();

   assert_eq!(card.url, "https://github.com/mobile?utm_source=x");
   assert_eq!(card.title, "GitHub Mobile");
   assert_eq!(
      card.description,
      "The world's development platform, in your pocket"
   );
   assert_eq!(card.type_, "link");
   assert_eq!((card.width, card.height), (800, 419));
   assert_eq!(card.provider_name, "github.com");
   assert!(
      card
         .image
         .is_some_and(|image| image.starts_with("https://teapot.test/pic/enc/"))
   );
   assert_eq!(activity.media_attachments.len(), 1);
   let attachment = &activity.media_attachments[0];
   assert_eq!(attachment.type_, "image");
   assert_eq!(attachment.description.as_deref(), Some("GitHub Mobile"));
   assert_eq!(
      attachment
         .meta
         .as_ref()
         .map(|meta| (meta.original.width, meta.original.height,)),
      Some((800, 419))
   );
}

#[test]
fn activity_payload_renders_poll_and_quote_blocks() {
   let mut status = tweet(405, "poller", "pick one");
   status.poll = Some(Poll {
      options: vec!["first".to_owned(), "second".to_owned()],
      values: vec![3, 1],
      votes: 4,
      ..Poll::default()
   });
   let quote = tweet(406, "quoted", "quoted text");
   status.quote = Some(Box::new(quote));

   let activity = build_activity_pub(&status, &test_config());

   assert!(
      activity
         .content
         .contains("<blockquote>████████████████████████")
   );
   assert!(activity.content.contains("<b>first</b>&emsp;75%"));
   assert!(
      activity
         .content
         .contains("4 votes · Final results</blockquote>")
   );
   assert!(
      activity
         .content
         .contains(r#"<a href="https://x.com/quoted/status/406">Quoting</a> quoted"#)
   );
}

#[test]
fn activity_payload_uses_image_attachment_for_transcoded_gif() {
   let mut tweet = tweet(407, "gif", "animated");
   tweet.gif = Some(Gif {
      url:      "https://video.twimg.com/tweet_video/animation.mp4".to_owned(),
      thumb:    "https://pbs.twimg.com/tweet_video_thumb/animation.jpg".to_owned(),
      alt_text: "animation alt text".to_owned(),
      width:    400,
      height:   400,
   });
   let mut config = test_config();
   config.gif_transcoding.mode = GifTranscodingMode::Local;

   let activity = build_activity_pub(&tweet, &config);
   let attachment = activity.media_attachments.first().unwrap();

   assert_eq!(attachment.type_, "image");
   assert!(attachment.url.starts_with("https://teapot.test/gif/"));
   assert_eq!(attachment.preview_url, None);
   assert_eq!(
      attachment.description.as_deref(),
      Some("animation alt text")
   );
}

#[test]
fn player_oembed_duplicates_engagement_in_provider() {
   let mut tweet = quoted_video_tweet();
   tweet.stats.views = 1_200;
   let html = render_tweet_embed(&tweet, &test_config(), true).into_string();

   assert!(html.contains("&amp;provider="));
   assert!(html.contains(r#"property="twitter:card" content="player""#));
   assert!(!html.contains(r#"property="og:video""#));
}

#[test]
fn activity_pub_photo_urls_are_encoded_and_keep_alt_text() {
   let mut tweet = tweet(300, "photos", "photo text");
   tweet.photos.push(Photo {
      url:      "https://pbs.twimg.com/media/photo.jpg?format=jpg&name=large".to_owned(),
      alt_text: "alt text".to_owned(),
      width:    2048,
      height:   1536,
   });

   let activity = build_activity_pub(&tweet, &test_config());
   let attachment = activity.media_attachments.first().unwrap();

   assert_eq!(attachment.type_, "image");
   assert!(
      attachment
         .url
         .starts_with("https://teapot.test/pic/orig/enc/")
   );
   assert_eq!(attachment.preview_url, None);
   assert_eq!(attachment.description.as_deref(), Some("alt text"));
   let dimensions = &attachment.meta.as_ref().unwrap().original;
   assert_eq!((dimensions.width, dimensions.height), (2048, 1536));
   assert_eq!(dimensions.size, "2048x1536");
   assert!((dimensions.aspect - 4.0 / 3.0).abs() < f64::EPSILON);
}

#[test]
fn activity_mixed_media_prioritizes_video_with_photo_poster() {
   let mut status = tweet(301, "mixed", "mixed media");
   let photo_url = "https://pbs.twimg.com/media/photo.jpg";
   status.photos.push(Photo {
      url: photo_url.to_owned(),
      width: 1200,
      height: 1600,
      ..Photo::default()
   });
   status.video = Some(video());
   let config = test_config();

   let activity = build_activity_pub(&status, &config);
   let expected_poster = format!(
      "https://teapot.test{}",
      formatters::get_pic_url(photo_url, config.config.base64_media),
   );

   assert_eq!(activity.media_attachments.len(), 2);
   assert_ne!(
      activity.media_attachments[0].id,
      activity.media_attachments[1].id
   );
   assert_eq!(activity.media_attachments[0].type_, "video");
   assert_eq!(activity.media_attachments[1].type_, "image");
   assert_eq!(
      activity.media_attachments[0].preview_url.as_deref(),
      Some(expected_poster.as_str()),
   );
}

#[test]
fn activity_pub_links_to_additional_videos() {
   let mut status = tweet(302, "videos", "two videos");
   status.video = Some(video());
   let mut second = video();
   second.thumb = "https://pbs.twimg.com/ext_tw_video_thumb/second.jpg".to_owned();
   second.variants[0].url =
      "https://video.twimg.com/ext_tw_video/2/pu/vid/1280x720/video.mp4".to_owned();
   status.additional_videos.push(second);

   let activity = build_activity_pub(&status, &test_config());

   assert_eq!(activity.media_attachments.len(), 1);
   assert_eq!(activity.media_attachments[0].type_, "video");
   assert!(activity.content.contains(
      r#"<sub>🗂️ <a href="https://teapot.test/videos/status/302">View all 2 media</a></sub><br>"#
   ));
}

#[test]
fn status_page_uses_author_first_embed_metadata() {
   let tweet = tweet(400, "G2CSGO", "Trying hard to win/lose/something");
   let html = render_status_page(
      &tweet,
      &html! {},
      &Prefs::default(),
      &test_config(),
      "G2CSGO",
      "400",
      false,
   )
   .into_string();

   assert!(!html.contains(
         r#"property="og:title" content="G2CSGO (@G2CSGO): &quot;Trying hard to win/lose/something&quot;""#
      ));
   assert!(html.contains(r#"property="twitter:title" content="G2CSGO (@G2CSGO)""#));
   assert!(html.contains(r#"property="og:title" content="G2CSGO (@G2CSGO)""#));
   assert!(
      html.contains(r#"property="og:description" content="Trying hard to win/lose/something""#)
   );
   assert!(html.contains(r#"property="twitter:site" content="@G2CSGO""#));
   assert!(html.contains(r#"property="og:url" content="https://x.com/G2CSGO/status/400""#));
   assert!(html.contains(r#"link rel="apple-touch-icon" href="https://teapot.test/pic/enc/"#));
   assert!(
      !html.contains(r#"link rel="apple-touch-icon" sizes="180x180" href="/apple-touch-icon.png""#)
   );
}

#[test]
fn discord_activity_page_defers_photo_media_to_mastodon_api() {
   let mut tweet = tweet(401, "photos", "photo text");
   tweet.time = time::OffsetDateTime::from_unix_timestamp(1).ok();
   tweet.photos.push(Photo {
      url: "https://pbs.twimg.com/media/photo.jpg".to_owned(),
      width: 2048,
      height: 1536,
      ..Photo::default()
   });

   let html = render_status_page(
      &tweet,
      &html! {},
      &Prefs::default(),
      &test_config(),
      "photos",
      "401",
      true,
   )
   .into_string();

   assert!(html.contains(r##"property="theme-color" content="#005ace""##));
   assert!(html.contains(r##"rel="mask-icon" href="/safari-pinned-tab.svg" color="#005ace""##));
   assert!(html.contains(
      r#"href="https://teapot.test/users/photos/statuses/401" type="application/activity+json""#
   ));
   assert!(html.contains(r#"property="twitter:card" content="summary_large_image""#));
   assert!(!html.contains(r#"property="og:image" content="https://teapot.test/pic/"#));
   assert!(!html.contains("article:published_time"));
}

#[test]
fn discord_activity_page_uses_avatar_for_text_only_status() {
   let tweet = tweet(402, "plain", "plain text");
   let html = render_status_page(
      &tweet,
      &html! {},
      &Prefs::default(),
      &test_config(),
      "plain",
      "402",
      true,
   )
   .into_string();

   assert!(html.contains(r#"property="og:image" content="https://teapot.test/pic/enc/"#));
   assert!(html.contains(r#"property="twitter:image" content="0""#));
   assert!(html.contains(r#"property="twitter:card" content="summary""#));
}

#[test]
fn generic_photo_metadata_includes_real_dimensions() {
   let mut tweet = tweet(403, "photos", "photo text");
   tweet.photos.push(Photo {
      url: "https://pbs.twimg.com/media/photo.jpg".to_owned(),
      width: 2048,
      height: 1536,
      ..Photo::default()
   });

   let html = render_status_page(
      &tweet,
      &html! {},
      &Prefs::default(),
      &test_config(),
      "photos",
      "403",
      false,
   )
   .into_string();

   assert!(html.contains(r#"property="og:image:width" content="2048""#));
   assert!(html.contains(r#"property="og:image:height" content="1536""#));
   assert!(html.contains(r#"property="twitter:image" content="https://teapot.test/pic/enc/"#));
   assert!(!html.contains("twitter:image:src"));
   assert!(!html.contains("application/activity+json"));
}

#[test]
fn activity_payload_matches_discord_mastodon_shape() {
   let mut tweet = tweet(404, "photos", "hello\nworld");
   tweet.stats.replies = 2;
   tweet.stats.retweets = 3;
   tweet.stats.likes = 4;
   tweet.stats.views = 5;
   tweet.lang = "en".to_owned();
   tweet.photos.extend([
      Photo {
         url: "https://pbs.twimg.com/media/one.jpg".to_owned(),
         width: 2048,
         height: 1536,
         ..Photo::default()
      },
      Photo {
         url: "https://pbs.twimg.com/media/two.jpg".to_owned(),
         width: 1024,
         height: 2048,
         ..Photo::default()
      },
   ]);

   let activity = build_activity_pub(&tweet, &test_config());

   assert_eq!(activity.id, "404");
   assert_eq!(activity.url, "https://x.com/photos/status/404");
   assert_eq!(activity.language, "en");
   assert_eq!(activity.media_attachments.len(), 2);
   assert_eq!(activity.media_attachments[0].preview_url, None);
   assert_eq!(
      activity.media_attachments[1]
         .meta
         .as_ref()
         .map(|meta| (meta.original.width, meta.original.height)),
      Some((1024, 2048))
   );
   assert!(activity.content.contains("hello<br>︀︀world<br><br><b>"));
   assert!(activity.content.contains("in_reply_to=404"));
   assert!(activity.content.contains("tweet_id=404"));
   assert_eq!(activity.account.avatar, activity.account.avatar_static);
   assert_eq!(activity.account.url, "https://x.com/photos");
   assert_eq!(activity.account.uri, "https://x.com/photos");
   assert!(activity.card.is_none());
   assert!(activity.poll.is_none());
}

#[test]
fn activity_media_with_missing_dimensions_uses_finite_fallback() {
   let mut tweet = tweet(405, "photos", "photo");
   tweet.photos.push(Photo {
      url: "https://pbs.twimg.com/media/no-size.jpg".to_owned(),
      ..Photo::default()
   });

   let activity = build_activity_pub(&tweet, &test_config());
   let dimensions = &activity.media_attachments[0]
      .meta
      .as_ref()
      .unwrap()
      .original;
   assert_eq!((dimensions.width, dimensions.height), (1200, 675));
   assert!(dimensions.aspect.is_finite());
}

#[test]
fn best_dimensions_parse_non_widescreen_video_url() {
   assert_eq!(video().best_dimensions(), (720, 1280));
}
