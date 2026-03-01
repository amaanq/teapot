use serde::{
   Deserialize,
   Serialize,
};

use super::User;
use crate::api::schema::CommunityNote;

/// Entity type for text expansion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityKind {
   #[default]
   Url,
   Mention,
   Hashtag,
   Symbol,
}

/// Text entity with position and content info.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Entity {
   /// Start and end byte indices in the text.
   pub indices: (usize, usize),
   /// Type of entity.
   pub kind:    EntityKind,
   /// Target URL for the link.
   pub url:     String,
   /// Display text for the link.
   pub display: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoType {
   #[default]
   #[serde(rename = "video/mp4")]
   Mp4,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoVariant {
   pub content_type: VideoType,
   pub url:          String,
   pub bitrate:      i32,
   pub resolution:   i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Video {
   pub duration_ms:   i32,
   pub url:           String,
   pub thumb:         String,
   pub available:     bool,
   pub reason:        String,
   pub title:         String,
   pub description:   String,
   pub playback_type: VideoType,
   pub variants:      Vec<VideoVariant>,
}

impl Video {
   /// Get the best MP4 URL for embedding.
   pub fn best_mp4_url(&self) -> Option<&str> {
      self
         .variants
         .iter()
         .filter(|variant| matches!(variant.content_type, VideoType::Mp4))
         .max_by_key(|variant| variant.resolution)
         .map(|variant| variant.url.as_str())
   }

   /// Get the best video dimensions.
   #[expect(
      clippy::cast_possible_truncation,
      reason = "aspect ratio calculation yields reasonable pixel values"
   )]
   #[expect(
      clippy::cast_precision_loss,
      reason = "video resolution i32 fits in f32 mantissa"
   )]
   pub fn best_dimensions(&self) -> (i32, i32) {
      self
         .variants
         .iter()
         .filter(|variant| matches!(variant.content_type, VideoType::Mp4))
         .max_by_key(|variant| variant.resolution)
         .map_or((1280, 720), |variant| {
            // Try to parse width from URL (format: /vid/WIDTHxHEIGHT/)
            // Default to 16:9 aspect ratio based on resolution
            let height = variant.resolution;
            let width = (height as f32 * 16.0 / 9.0) as i32;
            (width, height)
         })
   }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Gif {
   pub url:   String,
   pub thumb: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Photo {
   pub url:      String,
   pub alt_text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GalleryPhoto {
   pub url:      String,
   pub tweet_id: String,
   pub color:    String,
}

pub type PhotoRail = Vec<GalleryPhoto>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Poll {
   pub options:  Vec<String>,
   pub values:   Vec<i64>,
   pub votes:    i64,
   pub leader:   i64,
   #[serde(with = "time::serde::timestamp::option")]
   pub end_time: Option<time::OffsetDateTime>,
   pub image:    Option<String>,
}

impl Poll {
   /// Compute the poll status display string from `end_time`.
   /// Returns "Final results" for ended polls, or "N days/hours/minutes" for
   /// active ones.
   pub fn status_text(&self) -> String {
      let Some(end_time) = self.end_time else {
         return "Final results".to_owned();
      };
      let now = time::OffsetDateTime::now_utc();
      if end_time <= now {
         return "Final results".to_owned();
      }
      let diff = end_time - now;
      if diff.whole_days() > 0 {
         format!("{} days", diff.whole_days())
      } else if diff.whole_hours() > 0 {
         format!("{} hours", diff.whole_hours())
      } else {
         format!("{} minutes", diff.whole_minutes())
      }
   }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardKind {
   Amplify,
   App,
   #[serde(rename = "appplayer")]
   AppPlayer,
   Player,
   Summary,
   #[serde(rename = "summary_large_image")]
   SummaryLarge,
   #[serde(rename = "promo_website")]
   PromoWebsite,
   #[serde(rename = "promo_video_website")]
   PromoVideo,
   #[serde(rename = "promo_video_convo")]
   PromoVideoConvo,
   #[serde(rename = "promo_image_convo")]
   PromoImageConvo,
   #[serde(rename = "promo_image_app")]
   PromoImageApp,
   #[serde(rename = "direct_store_link_app")]
   StoreLink,
   #[serde(rename = "live_event")]
   LiveEvent,
   Broadcast,
   #[serde(rename = "periscope_broadcast")]
   Periscope,
   #[serde(rename = "unified_card")]
   Unified,
   Moment,
   #[serde(rename = "message_me")]
   MessageMe,
   #[serde(rename = "video_direct_message")]
   VideoDirectMessage,
   #[serde(rename = "image_direct_message")]
   ImageDirectMessage,
   Audiospace,
   #[serde(rename = "newsletter_publication")]
   NewsletterPublication,
   #[serde(rename = "job_details")]
   JobDetails,
   Hidden,
   #[default]
   Unknown,
}

impl CardKind {
   /// Parse a card kind from the Twitter API card name suffix.
   pub fn from_name(name: &str) -> Self {
      match name {
         "summary" => Self::Summary,
         "summary_large_image" => Self::SummaryLarge,
         "player" => Self::Player,
         "amplify" => Self::Amplify,
         "app" | "promo_image_app" => Self::App,
         "promo_image_convo" => Self::PromoImageConvo,
         "promo_video_website" => Self::PromoVideo,
         "promo_video_convo" => Self::PromoVideoConvo,
         "promo_website" => Self::PromoWebsite,
         "appplayer" => Self::AppPlayer,
         "direct_store_link_app" => Self::StoreLink,
         "live_event" => Self::LiveEvent,
         "broadcast" => Self::Broadcast,
         "periscope_broadcast" => Self::Periscope,
         "audiospace" => Self::Audiospace,
         "moment" => Self::Moment,
         "message_me" => Self::MessageMe,
         "video_direct_message" => Self::VideoDirectMessage,
         "image_direct_message" => Self::ImageDirectMessage,
         "newsletter_publication" => Self::NewsletterPublication,
         "unified_card" => Self::Unified,
         _ => Self::Unknown,
      }
   }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Card {
   pub kind:         CardKind,
   pub url:          String,
   pub title:        String,
   pub dest:         String,
   pub text:         String,
   pub image:        String,
   pub video:        Option<Video>,
   /// Raw member count for list/community cards. Views format the display
   /// string (e.g. "List · 42 Members").
   pub member_count: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TweetStats {
   pub replies:  i64,
   pub retweets: i64,
   pub likes:    i64,
   pub views:    i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Tweet {
   pub id:          i64,
   pub thread_id:   i64,
   pub reply_id:    i64,
   pub user:        User,
   pub text:        String,
   #[serde(with = "time::serde::timestamp::option")]
   pub time:        Option<time::OffsetDateTime>,
   pub reply:       Vec<String>,
   pub pinned:      bool,
   pub has_thread:  bool,
   pub available:   bool,
   pub tombstone:   String,
   pub location:    String,
   pub source:      String,
   pub stats:       TweetStats,
   pub retweet:     Option<Box<Self>>,
   pub attribution: Option<User>,
   pub media_tags:  Vec<User>,
   pub quote:       Option<Box<Self>>,
   pub card:        Option<Card>,
   pub poll:        Option<Poll>,
   pub gif:         Option<Gif>,
   pub video:       Option<Video>,
   pub photos:      Vec<Photo>,
   /// Community note (Birdwatch) -- structured data, rendered in views.
   pub note:        Option<CommunityNote>,
   /// Edit history tweet IDs.
   pub history:     Vec<i64>,
   /// Text entities for expansion (mentions, hashtags, URLs).
   pub entities:    Vec<Entity>,
}

impl Tweet {
   /// Check if this tweet has any media.
   pub const fn has_media(&self) -> bool {
      !self.photos.is_empty() || self.video.is_some() || self.gif.is_some()
   }
}
