use super::*;

/// Mastodon API v1-compatible status for Discord embed support.
/// Discord uses `created_at` for the footer timestamp and `content`
/// for rich text rendering (blockquotes for quoted tweets).
#[derive(Debug, Serialize)]
pub struct ActivityPubNote {
   pub id:                     String,
   pub url:                    String,
   pub uri:                    String,
   pub created_at:             String,
   pub edited_at:              Option<String>,
   pub reblog:                 Option<()>,
   pub in_reply_to_id:         Option<String>,
   pub in_reply_to_account_id: Option<String>,
   pub language:               String,
   pub content:                String,
   pub spoiler_text:           String,
   pub sensitive:              bool,
   pub visibility:             String,
   pub application:            MastodonApplication,
   pub media_attachments:      Vec<MediaAttachment>,
   pub account:                MastodonAccount,
   pub mentions:               Vec<()>,
   pub tags:                   Vec<()>,
   pub emojis:                 Vec<()>,
   pub card:                   Option<MastodonPreviewCard>,
   pub poll:                   Option<()>,
}

#[derive(Debug, Serialize)]
pub struct MastodonPreviewCard {
   pub url:           String,
   pub title:         String,
   pub description:   String,
   #[serde(rename = "type")]
   pub type_:         String,
   pub authors:       Vec<()>,
   pub author_name:   String,
   pub author_url:    String,
   pub provider_name: String,
   pub provider_url:  String,
   pub html:          String,
   pub width:         i32,
   pub height:        i32,
   pub image:         Option<String>,
   pub embed_url:     String,
   pub blurhash:      Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MastodonApplication {
   pub name:    String,
   pub website: Option<String>,
}

#[derive(Debug, Serialize)]
#[expect(
   clippy::struct_excessive_bools,
   reason = "mirrors the Mastodon API account schema Discord expects"
)]
pub struct MastodonAccount {
   pub id:               String,
   pub display_name:     String,
   pub username:         String,
   pub acct:             String,
   pub url:              String,
   pub uri:              String,
   pub created_at:       String,
   pub locked:           bool,
   pub bot:              bool,
   pub discoverable:     bool,
   pub indexable:        bool,
   pub group:            bool,
   pub avatar:           String,
   pub avatar_static:    String,
   #[serde(skip_serializing_if = "Option::is_none")]
   pub header:           Option<String>,
   #[serde(skip_serializing_if = "Option::is_none")]
   pub header_static:    Option<String>,
   pub followers_count:  i64,
   pub following_count:  i64,
   pub statuses_count:   i64,
   pub hide_collections: bool,
   pub noindex:          bool,
   pub emojis:           Vec<()>,
   pub roles:            Vec<()>,
   pub fields:           Vec<()>,
}

#[derive(Debug, Serialize)]
pub struct MediaAttachment {
   pub id:                 String,
   #[serde(rename = "type")]
   pub type_:              String,
   pub url:                String,
   pub preview_url:        Option<String>,
   pub remote_url:         Option<String>,
   pub preview_remote_url: Option<String>,
   pub text_url:           Option<String>,
   pub description:        Option<String>,
   pub meta:               Option<MediaMeta>,
}

#[derive(Debug, Serialize)]
pub struct MediaMeta {
   pub original: MediaDimensions,
}

#[derive(Debug, Serialize)]
pub struct MediaDimensions {
   pub width:  i32,
   pub height: i32,
   pub size:   String,
   pub aspect: f64,
}

/// Create a Mastodon-compatible media attachment.
fn make_attachment(
   id: String,
   type_: &str,
   url: String,
   preview: Option<String>,
   description: Option<String>,
   width: i32,
   height: i32,
) -> MediaAttachment {
   MediaAttachment {
      id,
      type_: type_.to_owned(),
      url,
      preview_url: preview,
      remote_url: None,
      preview_remote_url: None,
      text_url: None,
      description,
      meta: Some(MediaMeta {
         original: MediaDimensions {
            width,
            height,
            size: format!("{width}x{height}"),
            aspect: aspect_ratio(width, height),
         },
      }),
   }
}

const fn dimensions_or(width: i32, height: i32, fallback: (i32, i32)) -> (i32, i32) {
   if width > 0 && height > 0 {
      (width, height)
   } else {
      fallback
   }
}

fn full_media_url(url_prefix: &str, path: &str) -> String {
   format!("{url_prefix}{path}")
}

fn application_name(source: &str) -> String {
   let mut name = String::with_capacity(source.len());
   let mut in_tag = false;
   for character in source.chars() {
      match character {
         '<' => in_tag = true,
         '>' => in_tag = false,
         _ if !in_tag => name.push(character),
         _ => {},
      }
   }
   name
}

fn card_image_dimensions(url: &str) -> (i32, i32) {
   url.split(['?', '&'])
      .find_map(|part| {
         let dimensions = part.strip_prefix("name=")?;
         let (width, height) = dimensions.split_once('x')?;
         Some((width.parse().ok()?, height.parse().ok()?))
      })
      .unwrap_or_default()
}

fn build_preview_card(
   tweet: &Tweet,
   config: &Config,
   url_prefix: &str,
) -> Option<MastodonPreviewCard> {
   let card = tweet.card.as_ref().filter(|card| {
      !card.url.is_empty()
         && !matches!(card.kind, CardKind::Hidden | CardKind::Unknown)
         && tweet.poll.is_none()
   })?;
   let url = if card.url.starts_with("http://") || card.url.starts_with("https://") {
      card.url.clone()
   } else {
      format!("{url_prefix}{}", card.url)
   };
   let (width, height) = match card_image_dimensions(&card.image) {
      (0, 0) if matches!(card.kind, CardKind::Broadcast | CardKind::Periscope) => (1920, 1080),
      dimensions => dimensions,
   };
   let image = (!card.image.is_empty()).then(|| {
      full_media_url(
         url_prefix,
         &formatters::get_pic_url(&card.image, config.config.base64_media),
      )
   });

   Some(MastodonPreviewCard {
      url: url.clone(),
      title: card.title.clone(),
      description: card.text.clone(),
      type_: "link".to_owned(),
      authors: vec![],
      author_name: String::new(),
      author_url: String::new(),
      provider_name: card.dest.clone(),
      provider_url: url,
      html: String::new(),
      width,
      height,
      image,
      embed_url: String::new(),
      blurhash: None,
   })
}

/// Build media attachments for a single tweet's photos/video/gif.
fn build_media_attachments(
   tweet: &Tweet,
   config: &Config,
   url_prefix: &str,
) -> Vec<MediaAttachment> {
   let mut attachments = Vec::new();

   if let Some(video) = tweet.video.as_ref() {
      let (raw_width, raw_height) = video.best_dimensions();
      let scaled = formatters::scale_dimensions_for_embed(raw_width, raw_height);
      let (width, height) = dimensions_or(scaled.0, scaled.1, (1280, 720));
      if let Some(mp4_url) = video.best_mp4_url() {
         let poster = tweet
            .photos
            .first()
            .map_or(video.thumb.as_str(), |photo| photo.url.as_str());
         attachments.push(make_attachment(
            format!("{}-video-0", tweet.id),
            "video",
            full_media_url(
               url_prefix,
               &formatters::get_vid_url(
                  mp4_url,
                  &config.config.hmac_key,
                  config.config.base64_media,
               ),
            ),
            Some(full_media_url(
               url_prefix,
               &formatters::get_pic_url(poster, config.config.base64_media),
            )),
            None,
            width,
            height,
         ));
      }
   }

   for (index, photo) in tweet.photos.iter().enumerate() {
      let (width, height) = dimensions_or(photo.width, photo.height, (1200, 675));
      attachments.push(make_attachment(
         format!("{}-image-{index}", tweet.id),
         "image",
         full_media_url(
            url_prefix,
            &formatters::get_orig_pic_url(&photo.url, config.config.base64_media),
         ),
         None,
         (!photo.alt_text.is_empty()).then(|| photo.alt_text.clone()),
         width,
         height,
      ));
   }

   if let Some(ref gif) = tweet.gif
      && !gif.url.is_empty()
   {
      let (width, height) = dimensions_or(gif.width, gif.height, (480, 480));
      let description = (!gif.alt_text.is_empty()).then(|| gif.alt_text.clone());
      match config.gif_transcoding.mode {
         GifTranscodingMode::Local => {
            attachments.push(make_attachment(
               format!("{}-gif-0", tweet.id),
               "image",
               full_media_url(
                  url_prefix,
                  &formatters::get_gif_url(
                     &gif.url,
                     &config.config.hmac_key,
                     config.config.base64_media,
                  ),
               ),
               None,
               description,
               width,
               height,
            ));
         },
         GifTranscodingMode::External => {
            attachments.push(make_attachment(
               format!("{}-gif-0", tweet.id),
               "image",
               formatters::get_external_gif_url(&gif.url, &config.gif_transcoding.external_domain),
               None,
               description,
               width,
               height,
            ));
         },
         GifTranscodingMode::Off => {
            attachments.push(make_attachment(
               format!("{}-gif-0", tweet.id),
               "video",
               full_media_url(
                  url_prefix,
                  &formatters::get_vid_url(
                     &gif.url,
                     &config.config.hmac_key,
                     config.config.base64_media,
                  ),
               ),
               Some(full_media_url(
                  url_prefix,
                  &formatters::get_pic_url(&gif.thumb, config.config.base64_media),
               )),
               description,
               width,
               height,
            ));
         },
      }
   }

   attachments
}

fn append_media_overflow(content: &mut String, tweet: &Tweet, config: &Config) {
   if tweet.additional_videos.is_empty() {
      return;
   }

   let count = usize::from(tweet.video.is_some())
      + tweet.additional_videos.len()
      + tweet.photos.len()
      + usize::from(tweet.gif.is_some());
   let url = format!(
      "{}/{}/status/{}",
      config.url_prefix(),
      tweet.user.username,
      tweet.id
   );
   let _ = write!(
      content,
      r#"<sub>🗂️ <a href="{}">View all {count} media</a></sub><br>"#,
      html_escape(&url)
   );
}

fn preserve_activity_spacing(html: &str) -> String {
   const MAX_RUN: usize = 64;

   let mut output = String::with_capacity(html.len());
   let mut spaces = 0_usize;
   let flush_spaces = |target: &mut String, run_length: usize| {
      if run_length == 1 {
         target.push(' ');
      } else if run_length > 1 {
         for index in 0..run_length.min(MAX_RUN) {
            if index % 2 == 0 {
               target.push_str("&nbsp;");
            } else {
               target.push(' ');
            }
         }
         if run_length > MAX_RUN {
            target.push(' ');
         }
      }
   };

   for character in html.chars() {
      if character == ' ' {
         spaces += 1;
         continue;
      }
      flush_spaces(&mut output, spaces);
      spaces = 0;
      output.push(character);
   }
   flush_spaces(&mut output, spaces);
   output
}

fn activity_text(tweet: &Tweet) -> String {
   let expanded = expand_entities_for_x(tweet.text.trim(), &tweet.entities).replace('\n', "<br>︀︀");
   preserve_activity_spacing(&expanded)
}

fn activity_text_limited(tweet: &Tweet, max_chars: usize) -> String {
   let text = tweet.text.trim();
   if text.chars().count() <= max_chars {
      return activity_text(tweet);
   }

   let prefix = text.chars().take(max_chars).collect::<String>();
   let mut cutoff = prefix
      .rfind(char::is_whitespace)
      .map(|byte_index| prefix[..byte_index].chars().count())
      .filter(|&char_index| char_index >= max_chars * 3 / 4)
      .unwrap_or(max_chars);

   if let Some(entity) = tweet
      .entities
      .iter()
      .find(|entity| entity.indices.0 < cutoff && entity.indices.1 > cutoff)
   {
      cutoff = entity.indices.0;
   }

   let truncated = text.chars().take(cutoff).collect::<String>();
   let entities = tweet
      .entities
      .iter()
      .filter(|entity| entity.indices.1 <= cutoff)
      .cloned()
      .collect::<Vec<_>>();
   let expanded = expand_entities_for_x(truncated.trim_end(), &entities).replace('\n', "<br>︀︀");
   let expanded = preserve_activity_spacing(&expanded);
   format!("{expanded}…")
}

fn activity_text_with_reply_mentions(tweet: &Tweet) -> String {
   let text = activity_text(tweet);
   if !tweet.reply_mentions_stripped {
      return text;
   }
   let Some(first_reply) = tweet.reply.first() else {
      return text;
   };
   if tweet
      .text
      .trim_start()
      .get(..=first_reply.len())
      .is_some_and(|prefix| prefix.eq_ignore_ascii_case(&format!("@{first_reply}")))
   {
      return text;
   }

   let mentions = tweet
      .reply
      .iter()
      .map(|username| {
         let username = html_escape(username);
         format!(r#"<a href="https://x.com/{username}">@{username}</a>"#)
      })
      .collect::<Vec<_>>()
      .join(" ");
   format!("{mentions} {text}")
}

fn engagement_html(tweet: &Tweet) -> String {
   let status_id = tweet.id;
   let mut parts = Vec::new();

   if tweet.stats.replies > 0 {
      parts.push(format!(
         r#"<a href="https://x.com/intent/tweet?in_reply_to={status_id}">💬</a> {}"#,
         formatters::abbreviate_number(tweet.stats.replies)
      ));
   }
   if tweet.stats.retweets > 0 {
      parts.push(format!(
         r#"<a href="https://x.com/intent/retweet?tweet_id={status_id}">🔁</a> {}"#,
         formatters::abbreviate_number(tweet.stats.retweets)
      ));
   }
   if tweet.stats.likes > 0 {
      parts.push(format!(
         r#"<a href="https://x.com/intent/like?tweet_id={status_id}">❤️</a> {}"#,
         formatters::abbreviate_number(tweet.stats.likes)
      ));
   }
   if tweet.stats.views > 0 {
      parts.push(format!(
         "👁️ {}",
         formatters::abbreviate_number(tweet.stats.views)
      ));
   }

   if parts.is_empty() {
      String::new()
   } else {
      format!("<b>{}&ensp;</b>", parts.join("&ensp;"))
   }
}

fn append_poll(content: &mut String, tweet: &Tweet) {
   let Some(poll) = tweet.poll.as_ref() else {
      return;
   };

   let total_votes = if poll.votes > 0 {
      poll.votes
   } else {
      poll.values.iter().sum()
   };
   content.push_str("<blockquote>");

   for (option, votes) in poll.options.iter().zip(&poll.values) {
      let percentage = if total_votes > 0 {
         (votes.saturating_mul(100) + total_votes / 2) / total_votes
      } else {
         0
      }
      .clamp(0, 100);
      let bar_len = usize::try_from(percentage * 32 / 100).unwrap_or_default();
      let _ = write!(
         content,
         "{}<br><b>{}</b>&emsp;{percentage}%<br>︀︀︀<br>︀",
         "█".repeat(bar_len),
         html_escape(option),
      );
   }

   let _ = write!(
      content,
      "{} votes · {}</blockquote>",
      formatters::abbreviate_number(total_votes),
      html_escape(&poll.status_text()),
   );
}

fn append_quote(content: &mut String, tweet: &Tweet) {
   let Some(quote) = tweet.quote.as_ref() else {
      return;
   };

   let quote_url = format!("https://x.com/{}/status/{}", quote.user.username, quote.id);
   let profile_url = format!("https://x.com/{}", quote.user.username);
   let _ = write!(
      content,
      r#"<blockquote><b><a href="{quote_url}">Quoting</a> {} (<a href="{profile_url}">@{}</a>)</b><br>︀<br>{}</blockquote>"#,
      html_escape(&quote.user.fullname),
      html_escape(&quote.user.username),
      activity_text(quote),
   );
}

fn reply_context<'a>(tweet: &Tweet, replied_to: Option<&'a Tweet>) -> Option<&'a Tweet> {
   replied_to.filter(|original| {
      tweet.reply_id > 0 && original.id == tweet.reply_id && !original.text.trim().is_empty()
   })
}

fn append_reply_context(content: &mut String, original: &Tweet) {
   const MAX_CHARS: usize = 220;

   let status_url = format!(
      "https://x.com/{}/status/{}",
      original.user.username, original.id
   );
   let profile_url = format!("https://x.com/{}", original.user.username);
   let username = html_escape(&original.user.username);
   let _ = write!(
      content,
      r#"<blockquote><b>↩ <a href="{status_url}">Original post</a> · <a href="{profile_url}">@{username}</a></b><br>{}</blockquote>"#,
      activity_text_limited(original, MAX_CHARS),
   );
}

fn append_community_note(content: &mut String, tweet: &Tweet) {
   let Some(note) = tweet.note.as_ref() else {
      return;
   };

   let note_html = community_note_to_html(note).replace('\n', "<br>︀︀");
   let _ = write!(
      content,
      "<blockquote><b>📝 Community Note</b><br>{note_html}</blockquote>",
   );
}

fn append_article_preview(content: &mut String, tweet: &Tweet, config: &Config) {
   let Some(card) = tweet
      .card
      .as_ref()
      .filter(|card| card.kind == CardKind::Article && !card.title.is_empty())
   else {
      return;
   };

   if content == "<br><br>" {
      content.clear();
   }

   let url = if card.url.starts_with("http://") || card.url.starts_with("https://") {
      card.url.clone()
   } else {
      format!("{}{}", config.url_prefix(), card.url)
   };
   let description = preserve_activity_spacing(&html_escape(&card.text).replace('\n', "<br>"));
   let _ = write!(
      content,
      r#"<blockquote><b>📄 <a href="{}">{}</a></b>"#,
      html_escape(&url),
      html_escape(&card.title),
   );
   if !description.is_empty() {
      let _ = write!(content, "<br>{description}");
   }
   content.push_str("</blockquote>");
}

fn append_audio_space_preview(content: &mut String, tweet: &Tweet) {
   let Some(card) = tweet
      .card
      .as_ref()
      .filter(|card| card.kind == CardKind::Audiospace && !card.url.is_empty())
   else {
      return;
   };

   if content == "<br><br>" {
      content.clear();
   }

   let title = if card.title.is_empty() {
      "X Space"
   } else {
      &card.title
   };
   let _ = write!(
      content,
      r#"<blockquote><b>🎙️ <a href="{}">{}</a></b>"#,
      html_escape(&card.url),
      html_escape(title),
   );
   if !card.text.is_empty() {
      let _ = write!(content, "<br>{}", html_escape(&card.text));
   }
   if !card.dest.is_empty() {
      let _ = write!(content, "<br>{}", html_escape(&card.dest));
   }
   content.push_str("</blockquote>");
}

fn append_broadcast_preview(content: &mut String, tweet: &Tweet) {
   let Some(card) = tweet.card.as_ref().filter(|card| {
      matches!(card.kind, CardKind::Broadcast | CardKind::Periscope) && !card.url.is_empty()
   }) else {
      return;
   };

   if content == "<br><br>" {
      content.clear();
   }

   let title = if card.title.is_empty() {
      "Live broadcast"
   } else {
      &card.title
   };
   if broadcast_title_repeats_tweet(tweet, title) {
      content.push_str("<blockquote>📺");
      if !card.text.is_empty() {
         let _ = write!(content, " {}", html_escape(&card.text));
      }
      if !card.dest.is_empty() {
         let _ = write!(content, "<br>{}", html_escape(&card.dest));
      }
      content.push_str("</blockquote>");
      return;
   }
   let _ = write!(
      content,
      r#"<blockquote><b>📺 <a href="{}">{}</a></b>"#,
      html_escape(&card.url),
      html_escape(title),
   );
   if !card.text.is_empty() {
      let _ = write!(content, "<br>{}", html_escape(&card.text));
   }
   if !card.dest.is_empty() {
      let _ = write!(content, "<br>{}", html_escape(&card.dest));
   }
   content.push_str("</blockquote>");
}

/// Build rich Mastodon-status HTML for Discord's activity embed path.
fn build_mastodon_content(tweet: &Tweet, replied_to: Option<&Tweet>, config: &Config) -> String {
   let reply_context = reply_context(tweet, replied_to);
   let tweet_text = if reply_context.is_some() {
      activity_text(tweet)
   } else {
      activity_text_with_reply_mentions(tweet)
   };
   let mut content = tweet.translation.as_ref().map_or_else(
      || format!("{tweet_text}<br><br>"),
      |tl| {
         let translated = html_escape(&tl.text).replace('\n', "<br>");
         let translated = preserve_activity_spacing(&translated);
         format!(
            "<b>📑 Translated from {}</b><br><br>{}<br><br><blockquote><b>Original \
             text</b><br>{}</blockquote>",
            html_escape(&tl.source_lang_display),
            translated,
            tweet_text,
         )
      },
   );

   append_article_preview(&mut content, tweet, config);
   append_audio_space_preview(&mut content, tweet);
   append_broadcast_preview(&mut content, tweet);
   append_quote(&mut content, tweet);
   if let Some(original) = reply_context {
      append_reply_context(&mut content, original);
   }
   append_community_note(&mut content, tweet);

   if reply_context.is_none() && !tweet.reply.is_empty() {
      let username = html_escape(&tweet.reply[0]);
      content = format!(
         r#"<sub>↩ <a href="https://x.com/{username}" class="u-url mention"> (@{username})</a></sub><br>{content}"#
      );
   }

   append_poll(&mut content, tweet);
   append_media_overflow(&mut content, tweet, config);

   let engagement = engagement_html(tweet);
   if !engagement.is_empty() {
      content.push_str(&engagement);
   }

   content
}

/// Build Mastodon API v1-compatible status JSON for Discord.
pub fn build_activity_pub(tweet: &Tweet, config: &Config) -> ActivityPubNote {
   build_activity_pub_with_reply(tweet, None, config)
}

/// Build a Mastodon status with optional replied-to context for Discord.
pub fn build_activity_pub_with_reply(
   tweet: &Tweet,
   replied_to: Option<&Tweet>,
   config: &Config,
) -> ActivityPubNote {
   let url_prefix = config.url_prefix();

   let mut attachments = build_media_attachments(tweet, config, url_prefix);

   if !tweet.has_media()
      && let Some(ref quote) = tweet.quote
   {
      attachments.extend(build_media_attachments(quote, config, url_prefix));
   }

   let card = build_preview_card(tweet, config, url_prefix);

   // Discord ignores Mastodon's PreviewCard object, but does render media
   // attachments.
   if attachments.is_empty()
      && let Some(preview_card) = card.as_ref()
      && let Some(image) = preview_card.image.as_ref()
   {
      let (width, height) = dimensions_or(preview_card.width, preview_card.height, (800, 419));
      attachments.push(make_attachment(
         format!("{}-card-0", tweet.id),
         "image",
         image.clone(),
         None,
         Some(preview_card.title.clone()),
         width,
         height,
      ));
   }

   let created_at = tweet.time.map_or_else(
      || time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
      |ts| ts.format(&Rfc3339).unwrap(),
   );
   let account_created_at = tweet.user.join_date.map_or_else(
      || time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
      |ts| ts.format(&Rfc3339).unwrap(),
   );

   let status_url = format!("https://x.com/{}/status/{}", tweet.user.username, tweet.id);
   let profile_url = format!("https://x.com/{}", tweet.user.username);
   let avatar_url = formatters::get_pic_url(&tweet.user.user_pic, config.config.base64_media);
   let avatar_url = format!("{url_prefix}{avatar_url}");
   let header_url = (!tweet.user.banner.is_empty()).then(|| {
      format!(
         "{url_prefix}{}",
         formatters::get_pic_url(&tweet.user.banner, config.config.base64_media)
      )
   });

   ActivityPubNote {
      id: tweet.id.to_string(),
      url: status_url.clone(),
      uri: status_url,
      created_at,
      edited_at: None,
      reblog: None,
      in_reply_to_id: None,
      in_reply_to_account_id: None,
      language: if tweet.lang.is_empty() {
         "en".to_owned()
      } else {
         tweet.lang.clone()
      },
      content: build_mastodon_content(tweet, replied_to, config),
      spoiler_text: String::new(),
      sensitive: tweet.sensitive,
      visibility: "public".to_owned(),
      application: MastodonApplication {
         name:    application_name(&tweet.source),
         website: None,
      },
      media_attachments: attachments,
      account: MastodonAccount {
         id:               tweet.user.id.clone(),
         display_name:     tweet.user.fullname.clone(),
         username:         tweet.user.username.clone(),
         acct:             tweet.user.username.clone(),
         url:              profile_url.clone(),
         uri:              profile_url,
         created_at:       account_created_at,
         locked:           false,
         bot:              false,
         discoverable:     true,
         indexable:        false,
         group:            false,
         avatar:           avatar_url.clone(),
         avatar_static:    avatar_url,
         header:           header_url.clone(),
         header_static:    header_url,
         followers_count:  tweet.user.followers,
         following_count:  tweet.user.following,
         statuses_count:   tweet.user.tweets,
         hide_collections: false,
         noindex:          false,
         emojis:           vec![],
         roles:            vec![],
         fields:           vec![],
      },
      mentions: vec![],
      tags: vec![],
      emojis: vec![],
      card,
      poll: None,
   }
}

#[cfg(test)]
mod tests {
   use super::application_name;

   #[test]
   fn application_uses_source_label() {
      assert_eq!(
         application_name(
            r#"<a href="https://mobile.twitter.com" rel="nofollow">Twitter for iPhone</a>"#
         ),
         "Twitter for iPhone"
      );
   }
}
