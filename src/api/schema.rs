use std::{
   collections::HashMap,
   result,
};

use serde::{
   Deserialize,
   Deserializer,
   Serialize,
   de::IgnoredAny,
};

use crate::utils::formatters::parse_twitter_time;

// ── Shared enums ──────────────────────────────────────────────────────

/// Media type as returned by the Twitter API.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
   Photo,
   Video,
   AnimatedGif,
   #[serde(rename = "model3d")]
   Model3d,
   #[serde(other)]
   #[default]
   Unknown,
}

/// Timeline instruction types from the GraphQL API.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub enum InstructionType {
   TimelineAddEntries,
   TimelinePinEntry,
   TimelineReplaceEntry,
   TimelineClearCache,
   #[serde(other)]
   #[default]
   Other,
}

/// Video content type as returned by the API.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub enum RawVideoContentType {
   #[serde(rename = "video/mp4")]
   Mp4,
   #[serde(rename = "application/x-mpegURL")]
   M3u8,
   #[serde(other)]
   #[default]
   Other,
}

// ── Self-thread ────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct SelfThread {
   pub id_str: Option<String>,
}

// ── Edit control ───────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
#[expect(
   clippy::partial_pub_fields,
   reason = "serde struct with one public accessor"
)]
pub struct EditControl {
   edit_control_initial: Option<EditControlInner>,
   pub edit_tweet_ids:   Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
struct EditControlInner {
   edit_tweet_ids: Option<Vec<String>>,
}

// ── Birdwatch / Community note ─────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct BirdwatchPivot {
   pub subtitle: Option<BirdwatchSubtitle>,
}

#[derive(Deserialize, Default)]
pub struct BirdwatchSubtitle {
   pub text:     Option<String>,
   pub entities: Option<Vec<BirdwatchEntity>>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BirdwatchEntity {
   pub from_index: Option<u64>,
   pub to_index:   Option<u64>,
   #[serde(rename = "ref")]
   pub ref_data:   Option<BirdwatchRef>,
}

#[derive(Deserialize, Default)]
pub struct BirdwatchRef {
   pub url: Option<String>,
}

// ── Card wrapper ───────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct CardData {
   pub legacy:         Option<CardLegacy>,
   pub name:           Option<String>,
   pub url:            Option<String>,
   pub binding_values: BindingValues,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct CardLegacy {
   pub name:           Option<String>,
   pub url:            Option<String>,
   pub binding_values: BindingValues,
}

// ── Extended entities ──────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct ExtendedEntities {
   #[serde(default)]
   pub media: Vec<MediaItem>,
}

// ── Timeline instruction layer ──────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct Instruction {
   #[expect(
      clippy::struct_field_names,
      reason = "serde rename maps to API field name"
   )]
   #[serde(rename = "type")]
   pub instruction_type:    Option<InstructionType>,
   pub entries:             Option<Vec<Entry>>,
   #[serde(rename = "moduleItems")]
   pub module_items:        Option<Vec<Item>>,
   pub entry:               Option<Entry>,
   pub entry_id_to_replace: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct Entry {
   #[serde(rename = "entryId")]
   pub entry_id: Option<String>,
   pub content:  Option<EntryContent>,
}

#[derive(Deserialize, Default)]
pub struct EntryContent {
   #[serde(rename = "itemContent")]
   pub item_content: Option<ItemContent>,
   pub items:        Option<Vec<Item>>,
   pub value:        Option<String>,
}

#[derive(Deserialize, Default)]
pub struct Item {
   #[serde(rename = "entryId")]
   pub entry_id: Option<String>,
   pub item:     Option<ItemInner>,
}

#[derive(Deserialize, Default)]
pub struct ItemInner {
   #[serde(rename = "itemContent")]
   pub item_content: Option<ItemContent>,
   pub content:      Option<ItemContent>,
}

#[derive(Deserialize, Default)]
pub struct ItemContent {
   pub tweet_results:      Option<NestedResult<TweetData>>,
   pub user_results:       Option<NestedResult<UserData>>,
   #[serde(rename = "tweetDisplayType")]
   pub tweet_display_type: Option<String>,
   pub value:              Option<String>,
}

#[derive(Deserialize)]
pub struct NestedResult<T> {
   pub result: Option<Box<T>>,
}

impl<T> Default for NestedResult<T> {
   fn default() -> Self {
      Self { result: None }
   }
}

// Convenience methods on Entry

impl Entry {
   pub fn entry_id_str(&self) -> &str {
      self.entry_id.as_deref().unwrap_or("")
   }

   pub fn tweet_result(&self) -> Option<&TweetData> {
      self
         .content
         .as_ref()?
         .item_content
         .as_ref()?
         .tweet_results
         .as_ref()?
         .result
         .as_deref()
   }

   pub fn user_result(&self) -> Option<&UserData> {
      self
         .content
         .as_ref()?
         .item_content
         .as_ref()?
         .user_results
         .as_ref()?
         .result
         .as_deref()
   }

   pub fn cursor_value(&self) -> Option<&str> {
      let content = self.content.as_ref()?;
      content
         .value
         .as_deref()
         .or_else(|| content.item_content.as_ref()?.value.as_deref())
   }

   pub fn items(&self) -> &[Item] {
      self
         .content
         .as_ref()
         .and_then(|content| content.items.as_deref())
         .unwrap_or(&[])
   }
}

// Convenience methods on Item

impl Item {
   pub fn entry_id_str(&self) -> &str {
      self.entry_id.as_deref().unwrap_or("")
   }

   fn item_content(&self) -> Option<&ItemContent> {
      let inner = self.item.as_ref()?;
      inner.item_content.as_ref().or(inner.content.as_ref())
   }

   pub fn tweet_result(&self) -> Option<&TweetData> {
      self
         .item_content()?
         .tweet_results
         .as_ref()?
         .result
         .as_deref()
   }

   pub fn display_type(&self) -> Option<&str> {
      self.item_content()?.tweet_display_type.as_deref()
   }

   pub fn cursor_value(&self) -> Option<&str> {
      self.item_content()?.value.as_deref()
   }
}

// ── Entity types ────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct Entities {
   #[serde(default)]
   pub urls:          Vec<UrlEntity>,
   #[serde(default)]
   pub hashtags:      Vec<HashtagEntity>,
   #[serde(default)]
   pub user_mentions: Vec<MentionEntity>,
   #[serde(default)]
   pub symbols:       Vec<SymbolEntity>,
   #[serde(default)]
   pub media:         Vec<MediaItem>,
}

#[derive(Deserialize, Default)]
pub struct UrlEntity {
   #[serde(default)]
   pub indices:      Vec<u64>,
   pub url:          Option<String>,
   pub expanded_url: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct HashtagEntity {
   #[serde(default)]
   pub indices: Vec<u64>,
   pub text:    Option<String>,
}

#[derive(Deserialize, Default)]
pub struct MentionEntity {
   #[serde(default)]
   pub indices:     Vec<u64>,
   pub screen_name: Option<String>,
   pub name:        Option<String>,
}

#[derive(Deserialize, Default)]
pub struct SymbolEntity {
   #[serde(default)]
   pub indices: Vec<u64>,
   pub text:    Option<String>,
}

// ── Media types ─────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct MediaItem {
   #[serde(rename = "type")]
   pub media_type:             Option<MediaType>,
   pub media_url_https:        Option<String>,
   pub url:                    Option<String>,
   pub expanded_url:           Option<String>,
   pub video_info:             Option<VideoInfo>,
   pub ext_media_availability: Option<MediaAvailability>,
   pub additional_media_info:  Option<AdditionalMediaInfo>,
   pub ext_alt_text:           Option<String>,
}

#[derive(Deserialize, Default)]
pub struct AdditionalMediaInfo {
   pub title:       Option<String>,
   pub description: Option<String>,
   pub source_user: Option<Box<UserData>>,
}

#[derive(Deserialize, Default)]
pub struct MediaAvailability {
   pub status: Option<String>,
   pub reason: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct VideoInfo {
   pub duration_millis: i32,
   pub variants:        Vec<VideoVariant>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct VideoVariant {
   pub content_type: Option<RawVideoContentType>,
   pub bitrate:      i32,
   pub url:          Option<String>,
}

// ── Card binding values ─────────────────────────────────────────────────

/// Typed wrapper around the `[{key, value}]` binding-value array that Twitter
/// sends on card objects. Deserializes the array into a map and provides
/// `.string(key)` / `.image(key)` accessors so callers avoid raw map lookups.
#[derive(Deserialize, Default)]
#[serde(from = "Vec<BindingValueEntry>")]
pub struct BindingValues(HashMap<String, BindingValueInner>);

impl BindingValues {
   pub fn string(&self, key: &str) -> &str {
      self
         .0
         .get(key)
         .and_then(|bv| bv.string_value.as_deref())
         .unwrap_or_default()
   }

   pub fn image(&self, key: &str) -> &str {
      self
         .0
         .get(key)
         .and_then(|bv| bv.image_value.as_ref())
         .and_then(|img| img.url.as_deref())
         .unwrap_or_default()
   }

   pub fn is_empty(&self) -> bool {
      self.0.is_empty()
   }
}

impl From<Vec<BindingValueEntry>> for BindingValues {
   fn from(entries: Vec<BindingValueEntry>) -> Self {
      Self(
         entries
            .into_iter()
            .filter_map(|entry| Some((entry.key, entry.value?)))
            .collect(),
      )
   }
}

#[derive(Deserialize)]
pub struct BindingValueEntry {
   key:   String,
   value: Option<BindingValueInner>,
}

#[derive(Deserialize, Default)]
struct BindingValueInner {
   string_value: Option<String>,
   image_value:  Option<ImageValueInner>,
}

#[derive(Deserialize, Default)]
struct ImageValueInner {
   url: Option<String>,
}

// ── List type ───────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ListData {
   pub id_str:       Option<String>,
   pub rest_id:      Option<String>,
   pub name:         Option<String>,
   pub description:  Option<String>,
   pub member_count: i32,
   pub user_results: Option<NestedResult<UserData>>,
   #[serde(default, deserialize_with = "deser_banner_url")]
   pub banner_url:   Option<String>,
}

// ── Tweet types ─────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ContentDisclosure {
   pub advertising_disclosure:  Option<AdvertisingDisclosure>,
   pub ai_generated_disclosure: Option<AiGeneratedDisclosure>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct AdvertisingDisclosure {
   pub is_paid_promotion: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct AiGeneratedDisclosure {
   pub has_ai_generated_media: bool,
}

#[derive(Deserialize, Default)]
pub struct TweetData {
   #[expect(clippy::pub_underscore_fields, reason = "serde field name matches API")]
   pub __typename:              Option<String>,
   pub rest_id:                 Option<String>,
   pub legacy:                  Option<TweetLegacy>,
   pub core:                    Option<TweetCore>,
   #[serde(default, deserialize_with = "deser_note_tweet")]
   pub note_tweet:              Option<NoteTweet>,
   pub card:                    Option<CardData>,
   pub quoted_status_result:    Option<NestedResult<Self>>,
   #[serde(rename = "quotedPostResults")]
   pub quoted_post_results:     Option<NestedResult<Self>>,
   #[serde(rename = "repostedStatusResults")]
   pub reposted_status_results: Option<NestedResult<Self>>,
   pub views:                   Option<Views>,
   pub tweet:                   Option<Box<Self>>,
   #[serde(default, deserialize_with = "deser_tombstone")]
   pub tombstone:               Option<String>,
   pub reason:                  Option<String>,
   /// Fallback for reply-to tweet ID (newer GraphQL path).
   pub reply_to_results:        Option<ReplyToResults>,
   /// Fallback for reply-to user info (newer GraphQL path).
   #[serde(default, deserialize_with = "deser_reply_to_user")]
   pub reply_to_screen_name:    Option<String>,
   pub birdwatch_pivot:         Option<BirdwatchPivot>,
   pub edit_control:            Option<EditControl>,
   pub article:                 Option<ArticleWrapper>,
   pub content_disclosure:      Option<ContentDisclosure>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct TweetLegacy {
   pub id_str:                    Option<String>,
   pub full_text:                 Option<String>,
   pub text:                      Option<String>,
   pub created_at:                Option<String>,
   pub reply_count:               i64,
   pub retweet_count:             i64,
   pub favorite_count:            i64,
   pub quote_count:               i64,
   pub in_reply_to_status_id_str: Option<String>,
   pub in_reply_to_screen_name:   Option<String>,
   pub conversation_id_str:       Option<String>,
   pub self_thread:               Option<SelfThread>,
   pub retweeted_status_result:   Option<NestedResult<TweetData>>,
   pub display_text_range:        Option<Vec<u64>>,
   pub place:                     Option<Place>,
   pub withheld_in_countries:     Option<Vec<String>>,
   pub withheld_copyright:        Option<bool>,
   pub quoted_status_id_str:      Option<String>,
   pub is_quote_status:           Option<bool>,
   pub created_at_ms:             Option<i64>,
   pub entities:                  Option<Entities>,
   pub extended_entities:         Option<ExtendedEntities>,
}

#[derive(Deserialize, Default)]
pub struct TweetCore {
   pub user_results: Option<NestedResult<UserData>>,
   pub user_result:  Option<NestedResult<UserData>>,
}

#[derive(Deserialize, Default)]
pub struct NoteTweet {
   pub text:       Option<String>,
   pub entity_set: Option<Entities>,
}

#[derive(Deserialize, Default)]
pub struct Views {
   pub count: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct Place {
   pub full_name: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ReplyToResults {
   pub rest_id: Option<String>,
}

// ── User types ──────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct UserData {
   #[expect(clippy::pub_underscore_fields, reason = "serde field name matches API")]
   pub __typename:          Option<String>,
   pub rest_id:             Option<String>,
   pub is_blue_verified:    Option<bool>,
   #[serde(default, deserialize_with = "deser_present")]
   pub unavailable_message: bool,
   pub reason:              Option<String>,
   pub legacy:              Option<UserLegacy>,
   pub core:                Option<UserCore>,
   pub avatar:              Option<Avatar>,
   pub location:            Option<UserLocation>,
   pub profile_bio:         Option<ProfileBio>,
   pub verification:        Option<Verification>,
   pub privacy:             Option<Privacy>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct UserLegacy {
   pub id_str:                  Option<String>,
   pub screen_name:             Option<String>,
   pub name:                    Option<String>,
   pub description:             Option<String>,
   pub location:                Option<String>,
   pub profile_image_url_https: Option<String>,
   pub profile_banner_url:      Option<String>,
   pub profile_link_color:      Option<String>,
   pub followers_count:         i64,
   pub friends_count:           i64,
   pub statuses_count:          i64,
   pub favourites_count:        i64,
   pub media_count:             i64,
   pub protected:               Option<bool>,
   pub created_at:              Option<String>,
   pub verified_type:           Option<String>,
   pub pinned_tweet_ids_str:    Option<Vec<String>>,
   #[serde(default, deserialize_with = "deser_user_url_entities")]
   pub entities:                Vec<UrlEntity>,
}

#[derive(Deserialize, Default)]
pub struct UserCore {
   pub screen_name: Option<String>,
   pub name:        Option<String>,
   pub created_at:  Option<String>,
}

#[derive(Deserialize, Default)]
pub struct Avatar {
   pub image_url: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct UserLocation {
   pub location: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ProfileBio {
   pub description: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct Verification {
   pub verified_type: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Privacy {
   pub protected: bool,
}

// ── Unified card types ──────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct UnifiedCard {
   pub component_objects:   Option<HashMap<String, CardComponent>>,
   pub destination_objects: Option<HashMap<String, Destination>>,
   pub media_entities:      Option<HashMap<String, CardMediaEntity>>,
   #[serde(rename = "appStoreData")]
   pub app_store_data:      Option<HashMap<String, Vec<AppStoreEntry>>>,
}

#[derive(Deserialize, Default)]
pub struct CardComponent {
   #[serde(rename = "type")]
   pub comp_type: Option<String>,
   pub data:      Option<ComponentData>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ComponentData {
   pub destination:            Option<String>,
   pub title:                  Option<String>,
   pub name:                   Option<String>,
   pub member_count:           i32,
   pub id:                     Option<String>,
   pub media_list:             Option<Vec<MediaListItem>>,
   pub app_id:                 Option<String>,
   pub short_description_text: Option<String>,
   pub profile_user:           Option<ProfileUser>,
   pub location:               Option<String>,
   pub conversation_preview:   Option<Vec<ConversationMsg>>,
}

#[derive(Deserialize, Default)]
pub struct MediaListItem {
   pub id: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ProfileUser {
   pub username: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ConversationMsg {
   pub sender:  Option<String>,
   pub message: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct Destination {
   pub data: Option<DestinationData>,
}

#[derive(Deserialize, Default)]
pub struct DestinationData {
   pub url_data: Option<UrlDataInner>,
}

#[derive(Deserialize, Default)]
pub struct UrlDataInner {
   pub url:    Option<String>,
   pub vanity: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct CardMediaEntity {
   #[serde(rename = "type")]
   pub media_type:      Option<MediaType>,
   pub media_url_https: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct AppStoreEntry {
   #[serde(rename = "type")]
   pub app_type: Option<String>,
   pub id:       Option<String>,
   pub title:    Option<String>,
   pub category: Option<String>,
}

// ── Accessor methods ─────────────────────────────────────────────────────

impl TweetCore {
   pub fn user_value(&self) -> Option<&UserData> {
      let nr = self.user_results.as_ref().or(self.user_result.as_ref())?;
      nr.result.as_deref()
   }
}

impl EditControl {
   pub fn tweet_ids(&self) -> Option<&[String]> {
      self
         .edit_control_initial
         .as_ref()
         .and_then(|eci| eci.edit_tweet_ids.as_deref())
         .or(self.edit_tweet_ids.as_deref())
   }
}

impl TweetLegacy {
   pub fn is_withheld(&self) -> bool {
      self.withheld_copyright.unwrap_or(false)
         || self
            .withheld_in_countries
            .as_ref()
            .is_some_and(|countries| {
               countries
                  .iter()
                  .any(|cc| cc == "XX" || cc == "XY" || cc.to_lowercase().contains("withheld"))
            })
   }

   pub fn full_text(&self) -> &str {
      self
         .full_text
         .as_deref()
         .or(self.text.as_deref())
         .unwrap_or_default()
   }

   pub fn parse_time(&self) -> Option<time::OffsetDateTime> {
      self
         .created_at
         .as_deref()
         .and_then(parse_twitter_time)
         .or_else(|| {
            let ms = self.created_at_ms?;
            if ms == 0 {
               return None;
            }
            time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000).ok()
         })
   }

   pub fn reply_id(&self) -> i64 {
      self
         .in_reply_to_status_id_str
         .as_deref()
         .and_then(|id_str| id_str.parse().ok())
         .unwrap_or(0)
   }

   pub fn thread_id(&self, id: i64) -> i64 {
      let conv_id = self
         .conversation_id_str
         .as_deref()
         .and_then(|id_str| id_str.parse().ok())
         .unwrap_or(id);

      if self.self_thread.is_some() && conv_id == id {
         self
            .self_thread
            .as_ref()
            .and_then(|st| st.id_str.as_deref())
            .and_then(|id_str| id_str.parse().ok())
            .unwrap_or(conv_id)
      } else {
         conv_id
      }
   }

   pub fn location(&self) -> &str {
      self
         .place
         .as_ref()
         .and_then(|place| place.full_name.as_deref())
         .unwrap_or_default()
   }

   pub fn media_items(&self) -> &[MediaItem] {
      self
         .extended_entities
         .as_ref()
         .map(|ee| ee.media.as_slice())
         .or_else(|| self.entities.as_ref().map(|ent| ent.media.as_slice()))
         .unwrap_or_default()
   }

   pub fn expand_card_url(&self, tco: &str) -> Option<String> {
      self
         .entities
         .as_ref()?
         .urls
         .iter()
         .find(|url_ent| url_ent.url.as_deref() == Some(tco))
         .and_then(|url_ent| url_ent.expanded_url.clone())
   }
}

impl CardData {
   pub fn name(&self) -> &str {
      self
         .legacy
         .as_ref()
         .and_then(|leg| leg.name.as_deref())
         .or(self.name.as_deref())
         .unwrap_or_default()
   }

   pub fn url(&self) -> &str {
      self
         .legacy
         .as_ref()
         .and_then(|leg| leg.url.as_deref())
         .or(self.url.as_deref())
         .unwrap_or_default()
   }

   pub fn binding_values(&self) -> &BindingValues {
      self
         .legacy
         .as_ref()
         .map(|leg| &leg.binding_values)
         .filter(|bv| !bv.is_empty())
         .unwrap_or(&self.binding_values)
   }
}

/// A parsed community note with structured link data (no HTML).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommunityNote {
   pub text:  String,
   /// (`char_start`, `char_end`, url) triples for linked ranges.
   pub links: Vec<(usize, usize, String)>,
}

impl BirdwatchPivot {
   /// Extract structured community note data. HTML rendering is deferred to
   /// views.
   #[expect(
      clippy::cast_possible_truncation,
      reason = "entity indices fit in usize on any platform"
   )]
   pub fn to_note(&self) -> Option<CommunityNote> {
      let subtitle = self.subtitle.as_ref()?;
      let text = subtitle.text.as_deref()?.to_owned();
      if text.is_empty() {
         return None;
      }

      let mut links = Vec::new();
      if let Some(ref entities) = subtitle.entities {
         for entity in entities {
            let from = entity.from_index.unwrap_or(0) as usize;
            let to = entity.to_index.unwrap_or(0) as usize;
            let url = entity
               .ref_data
               .as_ref()
               .and_then(|ref_data| ref_data.url.as_deref())
               .unwrap_or_default();
            if !url.is_empty() && to > from {
               links.push((from, to, url.to_owned()));
            }
         }
      }

      Some(CommunityNote { text, links })
   }
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Deserializes any JSON value as `true`. Used with `#[serde(default)]`
/// so absent fields get `false`.
fn deser_present<'de, D: Deserializer<'de>>(de: D) -> result::Result<bool, D::Error> {
   IgnoredAny::deserialize(de).map(|_| true)
}

/// Flatten `{note_tweet_results: {result: NoteTweet}}` → `Option<NoteTweet>`.
fn deser_note_tweet<'de, D: Deserializer<'de>>(
   de: D,
) -> result::Result<Option<NoteTweet>, D::Error> {
   #[derive(Deserialize)]
   struct W1 {
      note_tweet_results: Option<W2>,
   }
   #[derive(Deserialize)]
   struct W2 {
      result: Option<NoteTweet>,
   }
   Ok(Option::<W1>::deserialize(de)?
      .and_then(|w| w.note_tweet_results)
      .and_then(|w| w.result))
}

/// Flatten `{text: {text: "..."}}` → `Option<String>`.
fn deser_tombstone<'de, D: Deserializer<'de>>(de: D) -> result::Result<Option<String>, D::Error> {
   #[derive(Deserialize)]
   struct Wrapper {
      text: Option<Inner>,
   }
   #[derive(Deserialize)]
   struct Inner {
      text: Option<String>,
   }
   Ok(Option::<Wrapper>::deserialize(de)?
      .and_then(|wrap| wrap.text)
      .and_then(|inner| inner.text))
}

/// Flatten `{result: {core: {screen_name: "..."}}}` → `Option<String>`.
fn deser_reply_to_user<'de, D: Deserializer<'de>>(
   de: D,
) -> result::Result<Option<String>, D::Error> {
   #[derive(Deserialize)]
   struct W1 {
      result: Option<W2>,
   }
   #[derive(Deserialize)]
   struct W2 {
      core: Option<W3>,
   }
   #[derive(Deserialize)]
   struct W3 {
      screen_name: Option<String>,
   }
   Ok(Option::<W1>::deserialize(de)?
      .and_then(|w| w.result)
      .and_then(|w| w.core)
      .and_then(|w| w.screen_name))
}

/// Flatten `{media_info: {original_img_url: "..."}}` → `Option<String>`.
fn deser_banner_url<'de, D: Deserializer<'de>>(de: D) -> result::Result<Option<String>, D::Error> {
   #[derive(Deserialize)]
   struct Wrapper {
      media_info: Option<Inner>,
   }
   #[derive(Deserialize)]
   struct Inner {
      original_img_url: Option<String>,
   }
   Ok(Option::<Wrapper>::deserialize(de)?
      .and_then(|wrap| wrap.media_info)
      .and_then(|inner| inner.original_img_url))
}

/// Flatten `{url: {urls: [...]}}` → `Vec<UrlEntity>`.
fn deser_user_url_entities<'de, D: Deserializer<'de>>(
   de: D,
) -> result::Result<Vec<UrlEntity>, D::Error> {
   #[derive(Deserialize)]
   struct Wrapper {
      url: Option<Inner>,
   }
   #[derive(Deserialize)]
   struct Inner {
      urls: Option<Vec<UrlEntity>>,
   }
   Ok(Option::<Wrapper>::deserialize(de)?
      .and_then(|wrap| wrap.url)
      .and_then(|inner| inner.urls)
      .unwrap_or_default())
}

/// Extract (start, end) from a raw indices array.
#[expect(
   clippy::cast_possible_truncation,
   reason = "entity indices fit in usize on any platform"
)]
pub fn indices(raw: &[u64]) -> (usize, usize) {
   (
      raw.first().copied().unwrap_or(0) as usize,
      raw.get(1).copied().unwrap_or(0) as usize,
   )
}

// ── GraphQL response envelope types ─────────────────────────────────────

/// Top-level GraphQL response wrapper. Every endpoint returns `{data: T}`.
#[derive(Deserialize)]
pub struct GqlResponse<T> {
   pub data: T,
}

/// `{timeline: {instructions: [...]}}` — shared by all timeline-shaped
/// endpoints.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct TimelinePayload {
   pub timeline: TimelineInstructions,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct TimelineInstructions {
   pub instructions: Vec<Instruction>,
}

// ── User endpoints (get_user, get_user_by_id) ──

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct UserResultData {
   pub user:         Option<NestedResult<UserData>>,
   pub user_result:  Option<NestedResult<UserData>>,
   pub user_results: Option<NestedResult<UserData>>,
}

// ── User timeline (get_user_tweets, get_user_media,
// get_user_tweets_and_replies) ──

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct UserTimelineData {
   pub user:        Option<TimelineNested>,
   pub user_result: Option<TimelineNested>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct TimelineNested {
   pub result: Option<TimelineResultData>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct TimelineResultData {
   pub timeline_v2:       Option<TimelinePayload>,
   pub timeline:          Option<TimelinePayload>,
   pub timeline_response: Option<TimelinePayload>,
}

impl TimelineResultData {
   pub fn instructions(&self) -> &[Instruction] {
      self
         .timeline_v2
         .as_ref()
         .or(self.timeline.as_ref())
         .or(self.timeline_response.as_ref())
         .map(|payload| payload.timeline.instructions.as_slice())
         .unwrap_or_default()
   }
}

// ── Search (search, search_users) ──

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct SearchTimelineData {
   pub search_by_raw_query: Option<SearchNested>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct SearchNested {
   pub search_timeline: Option<TimelinePayload>,
}

impl SearchTimelineData {
   pub fn instructions(&self) -> &[Instruction] {
      self
         .search_by_raw_query
         .as_ref()
         .and_then(|nested| nested.search_timeline.as_ref())
         .map(|payload| payload.timeline.instructions.as_slice())
         .unwrap_or_default()
   }
}

// ── Conversation (get_conversation) ──

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ConversationData {
   #[serde(rename = "tweetResult")]
   pub tweet_result:                             Option<NestedResult<TweetData>>,
   pub threaded_conversation_with_injections_v2: Option<TimelineInstructions>,
}

// ── List timeline (get_list_tweets) ──

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ListTimelineData {
   pub list: Option<ListTimelineNested>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ListTimelineNested {
   pub timeline_response: Option<TimelinePayload>,
}

impl ListTimelineData {
   pub fn instructions(&self) -> &[Instruction] {
      self
         .list
         .as_ref()
         .and_then(|nested| nested.timeline_response.as_ref())
         .map(|payload| payload.timeline.instructions.as_slice())
         .unwrap_or_default()
   }
}

// ── List members (get_list_members) ──

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ListMembersData {
   pub list: Option<ListMembersNested>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ListMembersNested {
   #[serde(alias = "membersTimeline")]
   pub members_timeline: Option<TimelinePayload>,
}

impl ListMembersData {
   pub fn instructions(&self) -> &[Instruction] {
      self
         .list
         .as_ref()
         .and_then(|nested| nested.members_timeline.as_ref())
         .map(|payload| payload.timeline.instructions.as_slice())
         .unwrap_or_default()
   }
}

// ── Retweeters (get_retweeters) ──

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct RetweetersData {
   pub retweeters_timeline: Option<TimelinePayload>,
}

impl RetweetersData {
   pub fn instructions(&self) -> &[Instruction] {
      self
         .retweeters_timeline
         .as_ref()
         .map(|payload| payload.timeline.instructions.as_slice())
         .unwrap_or_default()
   }
}

// ── List by ID (get_list) ──

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ListByIdData {
   pub list: Option<ListByIdWrapper>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ListByIdWrapper {
   #[serde(flatten)]
   pub data:   ListData,
   pub result: Option<Box<ListData>>,
}

impl ListByIdWrapper {
   pub fn list_data(&self) -> &ListData {
      self.result.as_deref().unwrap_or(&self.data)
   }
}

// ── List by slug (get_list_by_slug) ──

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ListBySlugData {
   pub user_by_screen_name: Option<ListBySlugNested>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ListBySlugNested {
   pub list: Option<ListData>,
}

// ── Edit history (get_edit_history) ──

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct EditHistoryData {
   pub tweet_result_by_rest_id: Option<EditHistoryNested>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct EditHistoryNested {
   pub result: Option<EditHistoryResult>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct EditHistoryResult {
   pub edit_history_timeline: Option<TimelinePayload>,
}

impl EditHistoryData {
   pub fn instructions(&self) -> &[Instruction] {
      self
         .tweet_result_by_rest_id
         .as_ref()
         .and_then(|nested| nested.result.as_ref())
         .and_then(|result| result.edit_history_timeline.as_ref())
         .map(|payload| payload.timeline.instructions.as_slice())
         .unwrap_or_default()
   }
}

// ── Article / Notes (inline in tweet response) ──

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ArticleWrapper {
   pub article_results: Option<NestedResult<InlineArticle>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct InlineArticle {
   pub rest_id:        Option<String>,
   pub title:          Option<String>,
   pub cover_media:    Option<InlineArticleCoverMedia>,
   pub media_entities: Option<Vec<ArticleMediaEntry>>,
   pub metadata:       Option<InlineArticleMetadata>,
   pub content_state:  Option<InlineContentState>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct InlineArticleCoverMedia {
   pub media_info: Option<InlineArticleCoverMediaInfo>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct InlineArticleCoverMediaInfo {
   pub original_img_url: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct InlineArticleMetadata {
   pub first_published_at_secs: Option<i64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct InlineContentState {
   pub blocks:     Vec<ArticleBlock>,
   #[serde(rename = "entityMap")]
   pub entity_map: Vec<EntityMapEntry>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct EntityMapEntry {
   pub key:   String,
   pub value: ArticleRawEntity,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ArticleBlock {
   pub text:                String,
   #[serde(rename = "type")]
   pub block_type:          String,
   #[serde(rename = "inlineStyleRanges")]
   pub inline_style_ranges: Vec<ArticleRawStyleRange>,
   #[serde(rename = "entityRanges")]
   pub entity_ranges:       Vec<ArticleRawEntityRange>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ArticleRawStyleRange {
   pub offset: usize,
   pub length: usize,
   pub style:  String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ArticleRawEntityRange {
   pub offset: usize,
   pub length: usize,
   pub key:    usize,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ArticleRawEntity {
   #[serde(rename = "type")]
   pub entity_type: String,
   pub data:        Option<ArticleRawEntityData>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ArticleRawEntityData {
   pub url:         Option<String>,
   pub markdown:    Option<String>,
   #[serde(rename = "mediaItems")]
   pub media_items: Option<Vec<ArticleRawMediaItem>>,
   #[serde(rename = "tweetId")]
   pub tweet_id:    Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ArticleRawMediaItem {
   #[serde(rename = "mediaId")]
   pub media_id: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ArticleMediaEntry {
   pub media_id:   Option<String>,
   pub media_info: Option<ArticleMediaInfo>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ArticleMediaInfo {
   #[expect(clippy::pub_underscore_fields, reason = "serde field name matches API")]
   pub __typename:       Option<String>,
   pub original_img_url: Option<String>,
   pub variants:         Option<Vec<ArticleMediaVariant>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ArticleMediaVariant {
   pub url: Option<String>,
}
