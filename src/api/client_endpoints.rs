use super::{
   ApiClient,
   Article,
   AudioSpaceData,
   BroadcastsData,
   CardKind,
   Conversation,
   ConversationData,
   Deserialize,
   Duration,
   EditHistory,
   EditHistoryData,
   Error,
   GalleryPhoto,
   List,
   ListByIdData,
   ListBySlugData,
   ListMembersData,
   ListTimelineData,
   PaginatedResult,
   Profile,
   Result,
   RetweetersData,
   SearchTimelineData,
   SessionKind,
   Timeline,
   Translation,
   Tweet,
   User,
   UserResultData,
   UserTimelineData,
   article_tweet_data,
   audio_space_host,
   audio_space_status,
   broadcast_id_from_url,
   broadcast_status,
   endpoints,
   header,
   hosted_by,
   parser,
   space_id_from_url,
   timeout,
};

#[expect(
   clippy::multiple_inherent_impl,
   reason = "endpoint methods are split from transport/authentication internals"
)]
impl ApiClient {
   /// Get user by screen name.
   pub async fn get_user(&self, screen_name: &str) -> Result<User> {
      let data = self
         .graphql_request::<UserResultData>(
            endpoints::GRAPH_USER,
            &endpoints::user_by_screen_name_vars(screen_name),
            endpoints::GQL_FEATURES,
            Some(endpoints::USER_FIELD_TOGGLES),
         )
         .await?;
      parser::parse_user(&data)
   }

   /// Get user by REST ID (numeric user ID).
   pub async fn get_user_by_id(&self, user_id: &str) -> Result<User> {
      if user_id.is_empty() || !user_id.chars().all(|ch| ch.is_ascii_digit()) {
         return Err(Error::UserNotFound("Invalid user ID format".to_owned()));
      }
      let data = self
         .graphql_request::<UserResultData>(
            endpoints::GRAPH_USER_BY_ID,
            &endpoints::user_by_id_vars(user_id),
            endpoints::GQL_FEATURES,
            Some(endpoints::USER_FIELD_TOGGLES),
         )
         .await?;
      parser::parse_user(&data)
   }

   /// Get edit history for a tweet.
   pub async fn get_edit_history(&self, tweet_id: &str) -> Result<EditHistory> {
      let data = self
         .graphql_request::<EditHistoryData>(
            endpoints::GRAPH_TWEET_EDIT_HISTORY,
            &endpoints::tweet_edit_history_vars(tweet_id),
            endpoints::GQL_FEATURES,
            None,
         )
         .await?;
      parser::edit_history::parse_edit_history(&data)
   }

   /// Get tweet by ID.
   ///
   /// Uses the `TweetDetail` endpoint (same as conversation) because
   /// `TweetResultByIdQuery` returns 404 for many tweets.
   pub async fn get_tweet(&self, tweet_id: &str) -> Result<Tweet> {
      let convo = self.get_conversation(tweet_id, None, "Relevance").await?;
      Ok(convo.tweet)
   }

   /// Re-fetch unavailable quote tweets (e.g. blocked-quoter tombstones).
   /// The tweet still exists but is hidden from the quoter's context.
   pub async fn resolve_unavailable_quote(&self, tweet: &mut Tweet) {
      let should_resolve = tweet
         .quote
         .as_ref()
         .is_some_and(|qt| !qt.available && qt.id != 0);
      if !should_resolve {
         return;
      }
      let quote_id = tweet.quote.as_ref().unwrap().id.to_string();
      if let Ok(resolved) = self.get_tweet(&quote_id).await {
         tweet.quote = Some(Box::new(resolved));
      }
   }

   async fn enrich_audio_space_card(&self, tweet: &mut Tweet) {
      let Some(space_id) = tweet
         .card
         .as_ref()
         .filter(|card| card.kind == CardKind::Audiospace)
         .and_then(|card| space_id_from_url(&card.url))
         .map(str::to_owned)
      else {
         return;
      };

      let Ok(data) = self
         .graphql_request::<AudioSpaceData>(
            endpoints::GRAPH_AUDIO_SPACE,
            &endpoints::audio_space_vars(&space_id),
            endpoints::GQL_FEATURES,
            None,
         )
         .await
      else {
         return;
      };
      let Some(metadata) = data.audio_space.and_then(|space| space.metadata) else {
         return;
      };
      let Some(card) = tweet
         .card
         .as_mut()
         .filter(|card| card.kind == CardKind::Audiospace)
      else {
         return;
      };

      if let Some(title) = metadata.title.as_deref().filter(|title| !title.is_empty()) {
         title.clone_into(&mut card.title);
      }
      card.text = audio_space_status(&metadata);
      card.dest = audio_space_host(&metadata);
   }

   async fn enrich_broadcast_card(&self, tweet: &mut Tweet) {
      let Some(broadcast_id) = tweet
         .card
         .as_ref()
         .filter(|card| matches!(card.kind, CardKind::Broadcast | CardKind::Periscope))
         .and_then(|card| broadcast_id_from_url(&card.url))
         .map(str::to_owned)
      else {
         return;
      };
      let url = endpoints::broadcast_show_url(&broadcast_id);
      let Ok(data) = self
         .cookie_json_request::<BroadcastsData>(
            endpoints::BROADCAST_SHOW_PATH,
            endpoints::BROADCAST_SHOW_PATH,
            &url,
         )
         .await
      else {
         return;
      };
      let Some(metadata) = data.broadcasts.get(&broadcast_id) else {
         return;
      };
      let Some(card) = tweet
         .card
         .as_mut()
         .filter(|card| matches!(card.kind, CardKind::Broadcast | CardKind::Periscope))
      else {
         return;
      };

      if !metadata.status.is_empty() {
         metadata.status.clone_into(&mut card.title);
      } else if !card.text.is_empty() {
         card.text.clone_into(&mut card.title);
      }
      card.text = broadcast_status(metadata);
      card.dest = hosted_by(&metadata.user_display_name, &metadata.twitter_username)
         .unwrap_or_else(|| "Live broadcast".to_owned());
      if !metadata.image_url.is_empty() {
         metadata.image_url.clone_into(&mut card.image);
         if let Some(video) = card.video.as_mut() {
            metadata.image_url.clone_into(&mut video.thumb);
         }
      }
   }

   /// Get conversation/thread for a tweet.
   pub async fn get_conversation(
      &self,
      tweet_id: &str,
      cursor: Option<&str>,
      ranking_mode: &str,
   ) -> Result<Conversation> {
      let data = self
         .graphql_request::<ConversationData>(
            endpoints::GRAPH_TWEET_DETAIL,
            &endpoints::tweet_detail_vars(tweet_id, cursor, ranking_mode),
            endpoints::GQL_FEATURES,
            Some(endpoints::TWEET_DETAIL_FIELD_TOGGLES),
         )
         .await?;
      let mut conversation = parser::parse_conversation(&data, tweet_id, cursor.is_some())?;
      if cursor.is_none()
         && let Some(tweet_data) = article_tweet_data(&data, tweet_id)
         && let Ok(article) = parser::parse_article(tweet_data)
      {
         parser::attach_article_preview(&mut conversation.tweet, &article);
      }
      if cursor.is_none()
         && conversation
            .tweet
            .entities
            .iter()
            .any(|entity| entity.url.contains("/article/"))
         && let Ok((_tweet, article)) = self.get_article_tweet(tweet_id).await
      {
         parser::attach_article_preview(&mut conversation.tweet, &article);
      }
      if cursor.is_none() {
         self.enrich_audio_space_card(&mut conversation.tweet).await;
         self.enrich_broadcast_card(&mut conversation.tweet).await;
      }
      Ok(conversation)
   }

   /// Get user's tweets timeline.
   pub async fn get_user_tweets(&self, user_id: &str, cursor: Option<&str>) -> Result<Timeline> {
      let data = self
         .graphql_request::<UserTimelineData>(
            endpoints::GRAPH_USER_TWEETS,
            &endpoints::user_tweets_vars(user_id, cursor),
            endpoints::GQL_FEATURES,
            Some(endpoints::USER_TWEETS_FIELD_TOGGLES),
         )
         .await?;
      parser::parse_timeline(&data)
   }

   /// Get user's media timeline.
   #[expect(
      clippy::significant_drop_tightening,
      reason = "session lease is transferred into the complete upstream request"
   )]
   pub async fn get_user_media(&self, user_id: &str, cursor: Option<&str>) -> Result<Timeline> {
      // Select the endpoint from the same leased session used for the request.
      let session = self
         .sessions
         .acquire(endpoints::GRAPH_USER_MEDIA, None)
         .await?;
      let (endpoint, variables) = match session.kind {
         SessionKind::OAuth => {
            (
               endpoints::GRAPH_USER_MEDIA_V2,
               endpoints::user_media_v2_vars(user_id, cursor),
            )
         },
         SessionKind::Cookie => {
            (
               endpoints::GRAPH_USER_MEDIA,
               endpoints::user_media_vars(user_id, cursor),
            )
         },
      };

      let data = self
         .graphql_request_with_session::<UserTimelineData>(
            session,
            endpoint,
            &variables,
            endpoints::GQL_FEATURES,
            None,
         )
         .await?;

      parser::parse_timeline(&data)
   }

   /// Get user's profile with tweets.
   pub async fn get_profile(&self, screen_name: &str, cursor: Option<&str>) -> Result<Profile> {
      // First get user info
      let user = self.get_user(screen_name).await?;

      // Protected/suspended accounts don't expose tweets
      if user.protected || user.suspended {
         return Ok(Profile {
            user,
            ..Profile::default()
         });
      }

      // Fetch tweets and photo rail in parallel (only for first page)
      let (tweets, photo_rail) = if cursor.is_none() {
         let tweets_future = self.get_user_tweets(&user.id, None);
         let photo_rail_future = self.get_photo_rail(&user.id);
         let (tweets_result, photo_rail_result) = tokio::join!(tweets_future, photo_rail_future);
         (tweets_result?, photo_rail_result.unwrap_or_default())
      } else {
         (self.get_user_tweets(&user.id, cursor).await?, Vec::new())
      };

      // Get pinned tweet if present
      let pinned = if user.pinned_tweet > 0 {
         tweets
            .content
            .iter()
            .flatten()
            .find(|tweet| tweet.id == user.pinned_tweet)
            .cloned()
      } else {
         None
      };

      Ok(Profile {
         user,
         photo_rail,
         pinned,
         tweets,
      })
   }

   /// Search tweets.
   pub async fn search(
      &self,
      query: &str,
      cursor: Option<&str>,
      product: &str,
   ) -> Result<Timeline> {
      let data = self
         .graphql_request::<SearchTimelineData>(
            endpoints::GRAPH_SEARCH_TIMELINE,
            &endpoints::search_vars(query, cursor, product),
            endpoints::GQL_FEATURES,
            None,
         )
         .await?;
      let mut timeline = parser::parse_search_timeline(&data);

      // When no more items are available the API returns the last page again.
      // Detect this by comparing the first 64 chars of the input and output cursors.
      if let Some(after) = cursor
         && let Some(ref bottom) = timeline.bottom
         && let Some(after_prefix) = after.get(..64)
         && let Some(bottom_prefix) = bottom.get(..64)
         && after_prefix == bottom_prefix
      {
         timeline.content.clear();
         timeline.bottom = None;
      }

      Ok(timeline)
   }

   /// Search users.
   pub async fn search_users(
      &self,
      query: &str,
      cursor: Option<&str>,
   ) -> Result<PaginatedResult<User>> {
      let data = self
         .graphql_request::<SearchTimelineData>(
            endpoints::GRAPH_SEARCH_TIMELINE,
            &endpoints::search_vars(query, cursor, "People"),
            endpoints::GQL_FEATURES,
            None,
         )
         .await?;
      Ok(parser::parse_user_search(&data))
   }

   /// Get list by ID.
   pub async fn get_list(&self, list_id: &str) -> Result<List> {
      let data = self
         .graphql_request::<ListByIdData>(
            endpoints::GRAPH_LIST_BY_ID,
            &endpoints::list_by_id_vars(list_id),
            endpoints::GQL_FEATURES,
            None,
         )
         .await?;
      let wrapper = data
         .list
         .as_ref()
         .ok_or_else(|| Error::NotFound("List not found".into()))?;
      Ok(parser::parse_list(wrapper.list_data()))
   }

   /// Get list by owner username and slug.
   pub async fn get_list_by_slug(&self, screen_name: &str, slug: &str) -> Result<List> {
      let data = self
         .graphql_request::<ListBySlugData>(
            endpoints::GRAPH_LIST_BY_SLUG,
            &endpoints::list_by_slug_vars(screen_name, slug),
            endpoints::GQL_FEATURES,
            None,
         )
         .await?;
      data
         .user_by_screen_name
         .as_ref()
         .and_then(|nested| nested.list.as_ref())
         .map(|ld| Ok(parser::parse_list(ld)))
         .ok_or_else(|| Error::NotFound("List not found".into()))?
   }

   /// Get list tweets.
   pub async fn get_list_tweets(&self, list_id: &str, cursor: Option<&str>) -> Result<Timeline> {
      let data = self
         .graphql_request::<ListTimelineData>(
            endpoints::GRAPH_LIST_TWEETS,
            &endpoints::list_timeline_vars(list_id, cursor),
            endpoints::GQL_FEATURES,
            None,
         )
         .await?;
      parser::parse_list_timeline(&data)
   }

   /// Get list members.
   pub async fn get_list_members(
      &self,
      list_id: &str,
      cursor: Option<&str>,
   ) -> Result<PaginatedResult<User>> {
      let data = self
         .graphql_request::<ListMembersData>(
            endpoints::GRAPH_LIST_MEMBERS,
            &endpoints::list_members_vars(list_id, cursor),
            endpoints::GQL_FEATURES,
            None,
         )
         .await?;
      Ok(parser::parse_list_members(&data))
   }

   /// Get users who retweeted a tweet.
   pub async fn get_retweeters(
      &self,
      tweet_id: &str,
      cursor: Option<&str>,
   ) -> Result<PaginatedResult<User>> {
      let data = self
         .graphql_request::<RetweetersData>(
            endpoints::GRAPH_RETWEETERS,
            &endpoints::retweeters_vars(tweet_id, cursor),
            endpoints::GQL_FEATURES,
            None,
         )
         .await?;
      Ok(parser::parse_retweeters(&data))
   }

   /// Get user's tweets and replies timeline.
   #[expect(
      clippy::significant_drop_tightening,
      reason = "session lease is transferred into the complete upstream request"
   )]
   pub async fn get_user_tweets_and_replies(
      &self,
      user_id: &str,
      cursor: Option<&str>,
   ) -> Result<Timeline> {
      let session = self
         .sessions
         .acquire(endpoints::GRAPH_USER_TWEETS_AND_REPLIES, None)
         .await?;
      let (endpoint, variables, field_toggles) = match session.kind {
         SessionKind::OAuth => {
            (
               endpoints::GRAPH_USER_TWEETS_AND_REPLIES_V2,
               endpoints::user_tweets_and_replies_v2_vars(user_id, cursor),
               None,
            )
         },
         SessionKind::Cookie => {
            (
               endpoints::GRAPH_USER_TWEETS_AND_REPLIES,
               endpoints::user_tweets_and_replies_vars(user_id, cursor),
               Some(endpoints::USER_TWEETS_FIELD_TOGGLES),
            )
         },
      };
      let data = self
         .graphql_request_with_session::<UserTimelineData>(
            session,
            endpoint,
            &variables,
            endpoints::GQL_FEATURES,
            field_toggles,
         )
         .await?;
      parser::parse_timeline(&data)
   }

   /// Get a tweet with its inline article data.
   ///
   /// Uses `TweetDetail` (not `TweetResultByIdQuery`) because only the detail
   /// endpoint supports `withArticleRichContentState`.
   pub async fn get_article_tweet(&self, tweet_id: &str) -> Result<(Tweet, Article)> {
      let data = self
         .graphql_request::<ConversationData>(
            endpoints::GRAPH_TWEET_DETAIL,
            &endpoints::tweet_detail_vars(tweet_id, None, "Relevance"),
            endpoints::GQL_FEATURES,
            Some(endpoints::TWEET_DETAIL_FIELD_TOGGLES),
         )
         .await?;

      // Parse the conversation to get the Tweet (reuses proven logic).
      let conversation = parser::parse_conversation(&data, tweet_id, false)?;
      let mut tweet = conversation.tweet;

      let tweet_data = article_tweet_data(&data, tweet_id)
         .ok_or_else(|| Error::TweetNotFound("Tweet data not found in response".into()))?;

      let article = parser::parse_article(tweet_data)?;
      parser::attach_article_preview(&mut tweet, &article);
      Ok((tweet, article))
   }

   /// Translate a tweet using the Strato translation API.
   pub async fn translate_tweet(&self, tweet_id: &str) -> Result<Translation> {
      let url = endpoints::translate_url(tweet_id);
      let session = self
         .sessions
         .acquire(endpoints::GRAPH_TWEET_DETAIL, Some(SessionKind::Cookie))
         .await?;

      let api_path = format!(
         "/i/api/1.1/strato/column/None/tweetId={tweet_id},destinationLanguage=None,\
          translationSource=Some(Google),feature=None,timeout=None,onlyCached=None/translation/\
          service/translateTweet"
      );
      let (bearer, tid) = self.bearer_and_tid(&api_path).await;

      let mut headers = header::HeaderMap::new();
      headers.insert(
         header::AUTHORIZATION,
         header::HeaderValue::from_str(bearer)
            .map_err(|_| Error::Internal("invalid bearer token value".into()))?,
      );
      headers.insert(
         "x-twitter-auth-type",
         header::HeaderValue::from_static("OAuth2Session"),
      );
      headers.insert(
         "x-csrf-token",
         session
            .ct0
            .parse()
            .map_err(|_| Error::Internal("invalid ct0 header value".into()))?,
      );
      headers.insert(
         header::COOKIE,
         format!("auth_token={}; ct0={}", session.auth_token, session.ct0)
            .parse()
            .map_err(|_| Error::Internal("invalid cookie header value".into()))?,
      );
      headers.insert(
         header::ORIGIN,
         header::HeaderValue::from_static("https://x.com"),
      );
      headers.insert(header::ACCEPT, header::HeaderValue::from_static("*/*"));
      headers.insert(
         "x-twitter-active-user",
         header::HeaderValue::from_static("yes"),
      );
      headers.insert(
         "x-twitter-client-language",
         header::HeaderValue::from_static("en"),
      );

      if let Some(tid) = tid
         && let Ok(val) = tid.parse()
      {
         headers.insert("x-client-transaction-id", val);
      }

      let response = self.client.get_with_headers(&url, &headers).await?;

      if !response.status().is_success() {
         let status = response.status();
         let body = response.text().await.unwrap_or_default();
         return Err(Error::Internal(format!(
            "Translation API error {status}: {body}"
         )));
      }

      #[expect(
         clippy::items_after_statements,
         reason = "local response type near its use"
      )]
      #[derive(Deserialize)]
      struct TranslationResponse {
         translation:               Option<String>,
         #[serde(rename = "sourceLanguage")]
         source_language:           Option<String>,
         #[serde(rename = "destinationLanguage")]
         dest_language:             Option<String>,
         #[serde(rename = "localizedSourceLanguage")]
         localized_source_language: Option<String>,
      }

      let bytes = response.bytes().await?;
      drop(session);
      let resp: TranslationResponse = serde_json::from_slice(&bytes)
         .map_err(|err| Error::Internal(format!("Translation parse error: {err}")))?;

      Ok(Translation {
         text:                resp.translation.unwrap_or_default(),
         source_lang:         resp.source_language.unwrap_or_default(),
         dest_lang:           resp.dest_language.unwrap_or_default(),
         source_lang_display: resp.localized_source_language.unwrap_or_default(),
      })
   }

   /// Translate a tweet using Kagi Translate API.
   pub async fn kagi_translate(&self, tweet: &Tweet, kagi_token: &str) -> Result<Translation> {
      use http_body_util::{
         BodyExt as _,
         Full,
         Limited,
      };
      use hyper_rustls::HttpsConnectorBuilder;
      use hyper_util::{
         client::legacy::Client as LegacyClient,
         rt::TokioExecutor,
      };
      use percent_encoding::{
         NON_ALPHANUMERIC,
         utf8_percent_encode,
      };

      #[derive(Deserialize)]
      struct KagiResponse {
         translation:       Option<String>,
         detected_language: Option<KagiDetectedLang>,
      }

      #[derive(Deserialize)]
      struct KagiDetectedLang {
         label: Option<String>,
      }

      if tweet.text.is_empty() {
         return Err(Error::Internal("Tweet has no text to translate".into()));
      }

      let payload = serde_json::json!({
         "text": tweet.text,
         "source_lang": tweet.lang,
         "target_lang": "en",
         "skip_definition": true,
         "model": "standard"
      });

      let url = format!(
         "https://translate.kagi.com/api/translate?token={}",
         utf8_percent_encode(kagi_token, NON_ALPHANUMERIC)
      );

      let request_body = payload.to_string();
      let uri: hyper::Uri = url
         .parse()
         .map_err(|err| Error::Internal(format!("invalid Kagi URL: {err}")))?;

      let connector = HttpsConnectorBuilder::new()
         .with_native_roots()
         .map_err(|err| Error::Internal(format!("TLS setup error: {err}")))?
         .https_only()
         .enable_http1()
         .build();

      let client = LegacyClient::builder(TokioExecutor::new()).build(connector);

      let request = hyper::Request::builder()
         .method(hyper::Method::POST)
         .uri(&uri)
         .header(header::HOST, "translate.kagi.com")
         .header(header::CONTENT_TYPE, "application/json")
         .body(Full::new(bytes::Bytes::from(request_body)))
         .map_err(|err| Error::Internal(format!("build Kagi request: {err}")))?;

      let resp = timeout(Duration::from_secs(30), client.request(request))
         .await
         .map_err(|_| Error::Internal("Kagi request timed out".into()))?
         .map_err(|err| Error::Internal(format!("Kagi request failed: {err}")))?;

      let status = resp.status();
      let response_body = Limited::new(resp.into_body(), 1024 * 1024);
      let body_bytes = timeout(Duration::from_secs(30), response_body.collect())
         .await
         .map_err(|_| Error::Internal("Kagi response body timed out".into()))?
         .map_err(|err| Error::Internal(format!("Kagi body read error: {err}")))?
         .to_bytes();

      if !status.is_success() {
         let body_text = String::from_utf8_lossy(&body_bytes);
         return Err(Error::Internal(format!(
            "Kagi API error {status}: {body_text}"
         )));
      }

      let kagi: KagiResponse = serde_json::from_slice(&body_bytes)
         .map_err(|err| Error::Internal(format!("Kagi parse error: {err}")))?;

      let source_display = kagi
         .detected_language
         .and_then(|dl| dl.label)
         .unwrap_or_else(|| tweet.lang.clone());

      Ok(Translation {
         text:                kagi.translation.unwrap_or_default(),
         source_lang:         tweet.lang.clone(),
         dest_lang:           "en".to_owned(),
         source_lang_display: source_display,
      })
   }

   /// Translate a tweet using the best available backend.
   /// Uses Kagi when a token is provided, otherwise falls back to Strato.
   pub async fn translate_auto(
      &self,
      tweet: &Tweet,
      kagi_token: Option<&str>,
   ) -> Result<Translation> {
      if let Some(token) = kagi_token {
         self.kagi_translate(tweet, token).await
      } else {
         self.translate_tweet(&tweet.id.to_string()).await
      }
   }

   /// Get session pool health statistics.
   pub async fn get_session_health(&self) -> super::super::HealthResponse {
      self.sessions.get_health().await
   }

   /// Get detailed session debug info.
   pub async fn get_session_debug(&self) -> super::super::DebugResponse {
      self.sessions.get_debug().await
   }

   /// Get photo rail (up to 16 recent photos) for a user.
   pub async fn get_photo_rail(&self, user_id: &str) -> Result<Vec<GalleryPhoto>> {
      let timeline = self.get_user_media(user_id, None).await?;

      let mut photos = Vec::new();
      for mut tweet in timeline.content.into_iter().flatten() {
         // Extract one photo from each tweet
         // first photo > video thumb > gif thumb > card image
         let url = if !tweet.photos.is_empty() {
            Some(tweet.photos.swap_remove(0).url)
         } else if let Some(video) = tweet.video.take() {
            (!video.thumb.is_empty()).then_some(video.thumb)
         } else if let Some(gif) = tweet.gif.take() {
            (!gif.thumb.is_empty()).then_some(gif.thumb)
         } else if let Some(card) = tweet.card.take() {
            (!card.image.is_empty()).then_some(card.image)
         } else {
            None
         };

         if let Some(url) = url {
            photos.push(GalleryPhoto {
               url,
               tweet_id: tweet.id.to_string(),
               color: String::new(),
            });
            if photos.len() >= 10 {
               return Ok(photos);
            }
         }
      }

      Ok(photos)
   }
}
