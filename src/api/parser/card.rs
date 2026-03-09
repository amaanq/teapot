use std::collections::HashMap;

use time::format_description::well_known::Rfc3339;

use crate::{
   api::schema::{
      BindingValues,
      CardData,
      Destination,
      MediaType,
      UnifiedCard,
   },
   error::{
      Error,
      Result,
   },
   types::{
      Card,
      CardKind,
      Poll,
      Video,
      VideoType,
      VideoVariant,
   },
};

/// Extract destination URL and vanity from typed destination objects.
fn extract_destination(
   dest_id: &str,
   destinations: &HashMap<String, Destination>,
) -> (String, String) {
   let Some(dest) = destinations.get(dest_id) else {
      return (String::new(), String::new());
   };
   let Some(url_data) = dest
      .data
      .as_ref()
      .and_then(|dest_data| dest_data.url_data.as_ref())
   else {
      return (String::new(), String::new());
   };
   let url = url_data.url.as_deref().unwrap_or("").to_owned();
   let vanity = url_data.vanity.as_deref().unwrap_or("").to_owned();
   (url, vanity)
}

impl TryFrom<&CardData> for Card {
   type Error = Error;

   fn try_from(card: &CardData) -> Result<Self> {
      let name = card.name();

      // Strip "poll2choice_" etc. prefix
      let kind_name = name.rsplit_once(':').map_or(name, |(_, suffix)| suffix);
      let kind = CardKind::from_name(kind_name);

      // Handle unified cards specially
      if kind == CardKind::Unified {
         return Ok(parse_unified_card(card.binding_values()));
      }

      let bv = card.binding_values();

      let url = card.url().to_owned();

      let title = bv.string("title").to_owned();
      let text = bv.string("description").to_owned();
      let dest = {
         let vanity = bv.string("vanity_url");
         if vanity.is_empty() {
            bv.string("domain").to_owned()
         } else {
            vanity.to_owned()
         }
      };

      // Try multiple image keys in order of preference (with _large suffix)
      let image = [
         "summary_photo_image_large",
         "player_image_large",
         "promo_image_large",
         "photo_image_full_size_large",
         "thumbnail_image_large",
         "thumbnail_large",
         "event_thumbnail_large",
         "image_large",
         // Fallbacks without _large suffix
         "thumbnail_image",
         "player_image",
         "summary_photo_image",
         "image",
      ]
      .iter()
      .map(|key| bv.image(key))
      .find(|img| !img.is_empty())
      .unwrap_or_default()
      .to_owned();

      // Card-type-specific logic
      let mut video = None;
      let mut card_url = url;
      let mut card_text = text;

      match kind {
         CardKind::PromoVideo
         | CardKind::PromoVideoConvo
         | CardKind::AppPlayer
         | CardKind::VideoDirectMessage => {
            video = Some(parse_promo_video(bv));
            if kind == CardKind::AppPlayer {
               let app_cat = bv.string("app_category");
               if !app_cat.is_empty() {
                  app_cat.clone_into(&mut card_text);
               }
            }
         },
         CardKind::Broadcast => {
            let broadcast_image = bv.image("broadcast_thumbnail_large").to_owned();
            return Ok(Self {
               kind,
               url: bv.string("broadcast_url").to_owned(),
               title: bv.string("broadcaster_display_name").to_owned(),
               dest,
               text: bv.string("broadcast_title").to_owned(),
               image: broadcast_image.clone(),
               member_count: 0,
               video: Some(Video {
                  thumb: broadcast_image,
                  available: true,
                  ..Default::default()
               }),
            });
         },
         CardKind::LiveEvent => {
            bv.string("event_title").clone_into(&mut card_text);
         },
         CardKind::Player => {
            let player_url = bv.string("player_url");
            if !player_url.is_empty() {
               // Convert YouTube embed URLs to watch URLs.
               // e.g. /embed/VIDEO_ID?list=LIST → /watch?v=VIDEO_ID&list=LIST
               let mut watch_url = player_url.replace("/embed/", "/watch?v=");
               // Fix double query string: ?v=X?param → ?v=X&param
               if let Some(second_q) = watch_url[1..].find('?') {
                  watch_url.replace_range(second_q + 1..second_q + 2, "&");
               }
               card_url = watch_url;
            }
         },
         _ => {},
      }

      // Clear URL for DM cards
      if matches!(
         kind,
         CardKind::VideoDirectMessage | CardKind::ImageDirectMessage
      ) {
         card_url = String::new();
      }

      Ok(Self {
         kind,
         url: card_url,
         title,
         dest,
         text: card_text,
         image,
         video,
         member_count: 0,
      })
   }
}

/// Parse promo video from card binding values.
/// Only MP4 URLs are supported; HLS/VMAP streams are skipped.
fn parse_promo_video(bv: &BindingValues) -> Video {
   let thumb = bv.image("player_image_large").to_owned();
   let duration_secs = bv.string("content_duration_seconds").parse().unwrap_or(0);

   // Only use direct MP4 stream URLs; skip HLS/VMAP since we don't support them
   let raw_stream = bv.string("player_stream_url");
   let stream_url =
      if !raw_stream.is_empty() && !raw_stream.contains("m3u8") && !raw_stream.contains("vmap") {
         raw_stream.to_owned()
      } else {
         String::new()
      };

   let available = !stream_url.is_empty();
   let variants = if available {
      vec![VideoVariant {
         content_type: VideoType::Mp4,
         url:          stream_url.clone(),
         bitrate:      0,
         resolution:   0,
      }]
   } else {
      Vec::new()
   };

   Video {
      duration_ms: duration_secs * 1000,
      url: stream_url,
      thumb,
      available,
      playback_type: VideoType::Mp4,
      variants,
      ..Default::default()
   }
}

/// Parse unified card (complex card format).
#[expect(
   clippy::too_many_lines,
   reason = "unified card parsing has inherent branching"
)]
fn parse_unified_card(bv: &BindingValues) -> Card {
   let raw_json = bv.string("unified_card");
   let unified_card_json = if raw_json.is_empty() { "{}" } else { raw_json };

   let card = serde_json::from_str::<UnifiedCard>(unified_card_json).unwrap_or_default();

   let mut result = Card {
      kind:         CardKind::Summary,
      url:          String::new(),
      title:        String::new(),
      dest:         String::new(),
      text:         String::new(),
      image:        String::new(),
      video:        None,
      member_count: 0,
   };

   let destinations = card.destination_objects.unwrap_or_default();
   let media_entities = card.media_entities.unwrap_or_default();
   let app_store_data = card.app_store_data.unwrap_or_default();

   if let Some(components) = card.component_objects {
      #[expect(clippy::iter_over_hash_type, reason = "component order is irrelevant")]
      for component in components.values() {
         let comp_type = component.comp_type.as_deref().unwrap_or("");
         let Some(data) = component.data.as_ref() else {
            continue;
         };

         match comp_type {
            "details" | "twitter_list_details" | "community_details" => {
               if let Some(dest_id) = data.destination.as_deref() {
                  let (url, vanity) = extract_destination(dest_id, &destinations);
                  result.url = url;
                  result.dest = vanity;
               }
               data
                  .title
                  .as_deref()
                  .or(data.name.as_deref())
                  .unwrap_or("")
                  .clone_into(&mut result.title);
               match comp_type {
                  "twitter_list_details" => {
                     result.member_count = data.member_count;
                     "List".clone_into(&mut result.dest);
                  },
                  "community_details" => {
                     result.member_count = data.member_count;
                     "Community".clone_into(&mut result.dest);
                  },
                  _ => {},
               }
            },
            "media" | "swipeable_media" => {
               let media_id = if comp_type == "swipeable_media" {
                  data
                     .media_list
                     .as_ref()
                     .and_then(|list| list.first())
                     .and_then(|media_ref| media_ref.id.as_deref())
               } else {
                  data.id.as_deref()
               };

               if let Some(mid) = media_id
                  && let Some(media) = media_entities.get(mid)
               {
                  if let Some(img_url) = media.media_url_https.as_deref() {
                     img_url.clone_into(&mut result.image);
                  }

                  match media.media_type.unwrap_or_default() {
                     MediaType::Photo => {
                        result.kind = CardKind::SummaryLarge;
                     },
                     MediaType::Video => {
                        result.kind = CardKind::PromoVideo;
                     },
                     MediaType::Model3d => {
                        "Unsupported 3D model".clone_into(&mut result.title);
                     },
                     _ => {},
                  }
               }
            },
            "app_store_details" => {
               if let Some(app_id) = data.app_id.as_deref()
                  && let Some(app_entry) = app_store_data
                     .get(app_id)
                     .and_then(|entries| entries.first())
               {
                  let app_type = app_entry.app_type.as_deref().unwrap_or("");
                  let app_store_id = app_entry.id.as_deref().unwrap_or("");

                  match app_type {
                     "android_app" => {
                        result.url =
                           format!("http://play.google.com/store/apps/details?id={app_store_id}");
                     },
                     "iphone_app" | "ipad_app" => {
                        result.url = format!("https://itunes.apple.com/app/id{app_store_id}");
                     },
                     _ => {},
                  }

                  app_entry
                     .title
                     .as_deref()
                     .unwrap_or("")
                     .clone_into(&mut result.title);
                  app_entry
                     .category
                     .as_deref()
                     .unwrap_or("")
                     .clone_into(&mut result.dest);
               }
            },
            "job_details" => {
               result.kind = CardKind::JobDetails;

               if let Some(dest_id) = data.destination.as_deref() {
                  let (url, _) = extract_destination(dest_id, &destinations);
                  result.url = url;
               }

               data
                  .title
                  .as_deref()
                  .unwrap_or("")
                  .clone_into(&mut result.title);
               data
                  .short_description_text
                  .as_deref()
                  .unwrap_or("")
                  .clone_into(&mut result.text);

               let username = data
                  .profile_user
                  .as_ref()
                  .and_then(|profile| profile.username.as_deref())
                  .unwrap_or("");
               let location = data.location.as_deref().unwrap_or("");
               result.dest = format!("@{username} · {location}");
            },
            "grok_share" => {
               result.kind = CardKind::SummaryLarge;

               if let Some(dest_id) = data.destination.as_deref() {
                  let (url, _) = extract_destination(dest_id, &destinations);
                  result.url = url;
               }

               "Answer by Grok".clone_into(&mut result.dest);

               let truncate = |text: &str, max: usize| {
                  let truncated: String = text.chars().take(max).collect();
                  if truncated.len() < text.len() {
                     format!("{truncated}...")
                  } else {
                     truncated
                  }
               };
               if let Some(conversation) = data.conversation_preview.as_ref() {
                  for msg in conversation {
                     match msg.sender.as_deref() {
                        Some("USER") => {
                           result.title = truncate(msg.message.as_deref().unwrap_or(""), 70);
                        },
                        Some("AGENT") => {
                           result.text = truncate(msg.message.as_deref().unwrap_or(""), 500);
                        },
                        _ => {},
                     }
                  }
               }
            },
            "hidden" => {
               result.kind = CardKind::Hidden;
            },
            "button_group" => {},
            _ => {
               tracing::debug!("Unknown unified card component type: {comp_type}");
            },
         }
      }
   }

   result
}

impl TryFrom<&CardData> for Poll {
   type Error = Error;

   fn try_from(card: &CardData) -> Result<Self> {
      let name = card.name();

      if !name.contains("poll") {
         return Err(Error::Internal("Not a poll card".into()));
      }

      let bv = card.binding_values();

      let mut options = Vec::new();
      let mut values = Vec::new();

      let has_choice_images = name.contains("image");
      let mut choice_images = Vec::new();

      for idx in 1..=4 {
         let label = bv.string(&format!("choice{idx}_label"));
         if label.is_empty() {
            break;
         }
         options.push(label.to_owned());

         let count = bv
            .string(&format!("choice{idx}_count"))
            .parse()
            .unwrap_or(0);
         values.push(count);

         if has_choice_images {
            let img = bv.image(&format!("choice{idx}_image_large"));
            if !img.is_empty() {
               choice_images.push(img.to_owned());
            }
         }
      }

      let votes = values.iter().sum();
      #[expect(
         clippy::cast_possible_wrap,
         reason = "poll option count is always tiny"
      )]
      let leader = values
         .iter()
         .enumerate()
         .max_by_key(|&(_, val)| val)
         .map_or(0, |(idx, _)| idx as i64);

      // Parse poll end time from end_datetime_utc (display formatting deferred
      // to views via Poll::status_text)
      let end_time = time::OffsetDateTime::parse(bv.string("end_datetime_utc"), &Rfc3339).ok();

      // Extract poll image if the card name contains "image"
      let image = if name.contains("image") {
         let img = bv.image("image_large");
         if img.is_empty() {
            None
         } else {
            Some(img.to_owned())
         }
      } else {
         None
      };

      Ok(Self {
         options,
         values,
         votes,
         leader,
         end_time,
         image,
         choice_images,
      })
   }
}
