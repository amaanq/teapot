use super::*;

// ── GraphQL response envelope types ─────────────────────────────────────

/// Top-level GraphQL response wrapper. Every endpoint returns `{data: T}`.
#[derive(Deserialize)]
pub struct GqlResponse<T> {
   pub data: T,
}

// ── X Spaces ──

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct AudioSpaceData {
   #[serde(rename = "audioSpace")]
   pub audio_space: Option<AudioSpace>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct AudioSpace {
   pub metadata: Option<AudioSpaceMetadata>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct AudioSpaceMetadata {
   pub rest_id:                       Option<String>,
   pub state:                         Option<String>,
   pub title:                         Option<String>,
   #[serde(default, deserialize_with = "deser_optional_i64")]
   pub scheduled_start:               Option<i64>,
   pub is_space_available_for_replay: Option<bool>,
   #[serde(default, deserialize_with = "deser_optional_i64")]
   pub total_live_listeners:          Option<i64>,
   #[serde(default, deserialize_with = "deser_optional_i64")]
   pub total_replay_watched:          Option<i64>,
   pub creator_results:               Option<NestedResult<UserData>>,
}

// ── Live broadcasts ──

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct BroadcastsData {
   pub broadcasts: HashMap<String, BroadcastMetadata>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct BroadcastMetadata {
   pub status:               String,
   pub image_url:            String,
   pub state:                String,
   pub user_display_name:    String,
   pub twitter_username:     String,
   pub available_for_replay: bool,
   #[serde(default, deserialize_with = "deser_optional_i64")]
   pub total_watching:       Option<i64>,
   #[serde(default, deserialize_with = "deser_optional_i64")]
   pub total_watched:        Option<i64>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum I64OrString {
   I64(i64),
   String(String),
}

fn deser_optional_i64<'de, D>(de: D) -> result::Result<Option<i64>, D::Error>
where
   D: Deserializer<'de>,
{
   Option::<I64OrString>::deserialize(de)?
      .map(|value| {
         match value {
            I64OrString::I64(value) => Ok(value),
            I64OrString::String(value) => value.parse().map_err(D::Error::custom),
         }
      })
      .transpose()
}

/// `{timeline: {instructions: [...]}}` wrapper shared by all timeline-shaped
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
