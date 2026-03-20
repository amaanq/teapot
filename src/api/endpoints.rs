/// Twitter/X API constants and endpoints.
use serde_json::json;

pub const CONSUMER_KEY: &str = "3nVuSoBZnx6U4vzUxf5w";
pub const CONSUMER_SECRET: &str = "Bcs59EFbbsdF6Sl9Ng71smgStWEGwXXKSjYvPVt7qys";
/// Bearer token that requires x-client-transaction-id (used for cookie sessions
/// with TID).
pub const BEARER_TOKEN: &str =
   "Bearer AAAAAAAAAAAAAAAAAAAAANRILgAAAAAAnNwIzUejRCOuH5E6I8xnZz4puTs%\
    3D1Zv7ttfk8LF81IUq16cHjhLTvJu4FA33AGWWjCpTnA";

/// Fallback bearer token that doesn't require x-client-transaction-id.
pub const BEARER_TOKEN_NO_TID: &str =
   "Bearer AAAAAAAAAAAAAAAAAAAAAFXzAwAAAAAAMHCxpeSDG1gLNLghVe8d74hl6k4%\
    3DRUMF4xAQLsbeBhTSRrCiQpJtxoGWeyHrDb5te2jpGskWDFW82F";

// GraphQL endpoints
pub const GRAPH_USER: &str = "-oaLodhGbbnzJBACb1kk2Q/UserByScreenName";
pub const GRAPH_USER_BY_ID: &str = "VN33vKXrPT7p35DgNR27aw/UserResultByIdQuery";
pub const GRAPH_USER_TWEETS: &str = "N9_71NodX1yntoC5pa4IFw/UserTweets";
pub const GRAPH_USER_MEDIA: &str = "36oKqyQ7E_9CmtONGjJRsA/UserMedia";
pub const GRAPH_USER_MEDIA_V2: &str = "bp0e_WdXqgNBIwlLukzyYA/MediaTimelineV2";
pub const GRAPH_TWEET_DETAIL: &str = "flqCy6kvOMolEquuRpOaHQ/TweetDetail";
pub const GRAPH_SEARCH_TIMELINE: &str = "bshMIjqDk8LTXTq4w91WKw/SearchTimeline";
pub const GRAPH_LIST_BY_ID: &str = "cIUpT1UjuGgl_oWiY7Snhg/ListByRestId";
pub const GRAPH_LIST_BY_SLUG: &str = "K6wihoTiTrzNzSF8y1aeKQ/ListBySlug";
pub const GRAPH_LIST_TWEETS: &str = "VQf8_XQynI3WzH6xopOMMQ/ListTimeline";
pub const GRAPH_LIST_MEMBERS: &str = "BQp2IEYkgxuSxqbTAr1e1g/ListMembers";
pub const GRAPH_USER_TWEETS_AND_REPLIES: &str = "kkaJ0Mf34PZVarrxzLihjg/UserTweetsAndReplies";
pub const GRAPH_USER_TWEETS_AND_REPLIES_V2: &str =
   "BDX77Xzqypdt11-mDfgdpQ/UserWithProfileTweetsAndRepliesQueryV2";
pub const GRAPH_TWEET_EDIT_HISTORY: &str = "upS9teTSG45aljmP9oTuXA/TweetEditHistory";
pub const GRAPH_RETWEETERS: &str = "tj-dlOvzRKjw69iy4z3LzQ/Retweeters";
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                              (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36";

// Base URLs
pub const GRAPHQL_URL: &str = "https://x.com/i/api/graphql";
pub const API_URL: &str = "https://api.x.com/graphql";
pub const STRATO_URL: &str = "https://x.com/i/api/1.1/strato/column/None";

/// GraphQL features to include in requests.
pub const GQL_FEATURES: &str = r#"{"android_ad_formats_media_component_render_overlay_enabled":false,"android_graphql_skip_api_media_color_palette":false,"android_professional_link_spotlight_display_enabled":false,"blue_business_profile_image_shape_enabled":false,"commerce_android_shop_module_enabled":false,"creator_subscriptions_subscription_count_enabled":false,"creator_subscriptions_tweet_preview_api_enabled":true,"freedom_of_speech_not_reach_fetch_enabled":true,"graphql_is_translatable_rweb_tweet_is_translatable_enabled":true,"hidden_profile_likes_enabled":false,"highlights_tweets_tab_ui_enabled":false,"interactive_text_enabled":false,"longform_notetweets_consumption_enabled":true,"longform_notetweets_inline_media_enabled":true,"longform_notetweets_rich_text_read_enabled":true,"longform_notetweets_richtext_consumption_enabled":true,"mobile_app_spotlight_module_enabled":false,"responsive_web_edit_tweet_api_enabled":true,"responsive_web_enhance_cards_enabled":false,"responsive_web_graphql_exclude_directive_enabled":true,"responsive_web_graphql_skip_user_profile_image_extensions_enabled":false,"responsive_web_graphql_timeline_navigation_enabled":true,"responsive_web_media_download_video_enabled":false,"responsive_web_text_conversations_enabled":false,"responsive_web_twitter_article_tweet_consumption_enabled":true,"unified_cards_destination_url_params_enabled":false,"responsive_web_twitter_blue_verified_badge_is_enabled":true,"rweb_lists_timeline_redesign_enabled":true,"spaces_2022_h2_clipping":true,"spaces_2022_h2_spaces_communities":true,"standardized_nudges_misinfo":true,"subscriptions_verification_info_enabled":true,"subscriptions_verification_info_reason_enabled":true,"subscriptions_verification_info_verified_since_enabled":true,"super_follow_badge_privacy_enabled":false,"super_follow_exclusive_tweet_notifications_enabled":false,"super_follow_tweet_api_enabled":false,"super_follow_user_api_enabled":false,"tweet_awards_web_tipping_enabled":false,"tweet_with_visibility_results_prefer_gql_limited_actions_policy_enabled":true,"tweetypie_unmention_optimization_enabled":false,"unified_cards_ad_metadata_container_dynamic_card_content_query_enabled":false,"verified_phone_label_enabled":false,"vibe_api_enabled":false,"view_counts_everywhere_api_enabled":true,"premium_content_api_read_enabled":false,"communities_web_enable_tweet_community_results_fetch":true,"responsive_web_jetfuel_frame":true,"responsive_web_grok_analyze_button_fetch_trends_enabled":false,"responsive_web_grok_image_annotation_enabled":true,"responsive_web_grok_imagine_annotation_enabled":true,"rweb_tipjar_consumption_enabled":true,"profile_label_improvements_pcf_label_in_post_enabled":true,"creator_subscriptions_quote_tweet_preview_enabled":false,"c9s_tweet_anatomy_moderator_badge_enabled":true,"responsive_web_grok_analyze_post_followups_enabled":true,"rweb_video_timestamps_enabled":false,"responsive_web_grok_share_attachment_enabled":true,"articles_preview_enabled":true,"immersive_video_status_linkable_timestamps":false,"articles_api_enabled":false,"responsive_web_grok_analysis_button_from_backend":true,"rweb_video_screen_enabled":false,"payments_enabled":false,"responsive_web_profile_redirect_enabled":false,"responsive_web_grok_show_grok_translated_post":false,"responsive_web_grok_community_note_auto_translation_is_enabled":false,"profile_label_improvements_pcf_label_in_profile_enabled":false,"grok_android_analyze_trend_fetch_enabled":false,"grok_translations_community_note_auto_translation_is_enabled":false,"grok_translations_post_auto_translation_is_enabled":false,"grok_translations_community_note_translation_is_enabled":false,"grok_translations_timeline_user_bio_auto_translation_is_enabled":false,"subscriptions_feature_can_gift_premium":false,"responsive_web_twitter_article_notes_tab_enabled":false,"subscriptions_verification_info_is_identity_verified_enabled":false,"hidden_profile_subscriptions_enabled":false,"content_disclosure_indicator_enabled":true,"content_disclosure_ai_generated_indicator_enabled":true}"#;

pub const USER_FIELD_TOGGLES: &str = r#"{"withPayments":false,"withAuxiliaryUserLabels":true}"#;
pub const USER_TWEETS_FIELD_TOGGLES: &str = r#"{"withArticlePlainText":false}"#;
pub const TWEET_DETAIL_FIELD_TOGGLES: &str = r#"{"withArticleRichContentState":true,"withArticlePlainText":false,"withGrokAnalyze":false,"withDisallowedReplyControls":false}"#;

// ── Helper ──────────────────────────────────────────────────────────────

/// Serialize a JSON value to string, stripping null fields (for optional
/// cursor).
fn vars(mut value: serde_json::Value) -> String {
   if let Some(obj) = value.as_object_mut() {
      obj.retain(|_, val| !val.is_null());
   }
   value.to_string()
}

// ── Builder functions ───────────────────────────────────────────────────

pub fn tweet_detail_vars(focal_tweet_id: &str, cursor: Option<&str>, ranking_mode: &str) -> String {
   vars(json!({
      "focalTweetId": focal_tweet_id, "cursor": cursor,
      "referrer": "profile", "withRuxInjections": false, "rankingMode": ranking_mode,
      "includePromotedContent": false, "withCommunity": true,
      "withQuickPromoteEligibilityTweetFields": true,
      "withBirdwatchNotes": true, "withVoice": true,
   }))
}

pub fn user_tweets_vars(user_id: &str, cursor: Option<&str>) -> String {
   vars(json!({
      "userId": user_id, "cursor": cursor, "count": 20,
      "includePromotedContent": false,
      "withQuickPromoteEligibilityTweetFields": true, "withVoice": true,
   }))
}

pub fn user_media_vars(user_id: &str, cursor: Option<&str>) -> String {
   vars(json!({
      "userId": user_id, "cursor": cursor, "count": 20,
      "includePromotedContent": false, "withClientEventToken": false,
      "withBirdwatchNotes": false, "withVoice": true,
   }))
}

pub fn user_media_v2_vars(user_id: &str, cursor: Option<&str>) -> String {
   vars(json!({ "rest_id": user_id, "cursor": cursor, "count": 20 }))
}

pub fn user_tweets_and_replies_v2_vars(user_id: &str, cursor: Option<&str>) -> String {
   vars(json!({ "rest_id": user_id, "cursor": cursor, "count": 20 }))
}

pub fn user_by_screen_name_vars(screen_name: &str) -> String {
   vars(json!({ "screen_name": screen_name, "withSafetyModeUserFields": true }))
}

pub fn user_by_id_vars(rest_id: &str) -> String {
   json!({ "rest_id": rest_id }).to_string()
}

pub fn list_members_vars(list_id: &str, cursor: Option<&str>) -> String {
   vars(json!({ "listId": list_id, "cursor": cursor, "count": 20 }))
}

pub fn user_tweets_and_replies_vars(user_id: &str, cursor: Option<&str>) -> String {
   vars(json!({
      "userId": user_id, "cursor": cursor, "count": 20,
      "includePromotedContent": false, "withCommunity": true, "withVoice": true,
   }))
}

pub fn list_by_slug_vars(screen_name: &str, list_slug: &str) -> String {
   json!({ "screenName": screen_name, "listSlug": list_slug }).to_string()
}

pub fn tweet_edit_history_vars(tweet_id: &str) -> String {
   json!({ "tweetId": tweet_id, "withQuickPromoteEligibilityTweetFields": true }).to_string()
}

pub fn retweeters_vars(tweet_id: &str, cursor: Option<&str>) -> String {
   vars(json!({
      "tweetId": tweet_id, "cursor": cursor, "count": 20,
      "includePromotedContent": false,
   }))
}

pub fn search_vars(raw_query: &str, cursor: Option<&str>, product: &str) -> String {
   vars(json!({
      "rawQuery": raw_query, "cursor": cursor, "count": 20,
      "querySource": "typedQuery", "product": product,
      "withDownvotePerspective": false, "withReactionsMetadata": false,
      "withReactionsPerspective": false,
   }))
}

pub fn list_by_id_vars(list_id: &str) -> String {
   json!({ "listId": list_id }).to_string()
}

pub fn list_timeline_vars(rest_id: &str, cursor: Option<&str>) -> String {
   vars(json!({ "rest_id": rest_id, "cursor": cursor, "count": 20 }))
}

/// Build the Strato translate tweet URL.
pub fn translate_url(tweet_id: &str) -> String {
   format!(
      "{STRATO_URL}/tweetId={tweet_id},destinationLanguage=None,translationSource=Some(Google),\
       feature=None,timeout=None,onlyCached=None/translation/service/translateTweet"
   )
}
