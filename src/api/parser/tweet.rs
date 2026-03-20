use super::{
   entity::parse_entities_from,
   media::parse_media,
   user::parse_user_object,
};
use crate::{
   api::schema::{
      TweetData,
      TweetLegacy,
      indices,
   },
   error::{
      Error,
      Result,
   },
   types::{
      Card,
      CardKind,
      EntityKind,
      Photo,
      Poll,
      Tweet,
      TweetStats,
   },
};

/// Parse a tweet object (from `NestedResult` or direct).
pub fn parse_tweet_object(raw: &TweetData) -> Result<Tweet> {
   Tweet::try_from(raw)
}

impl TryFrom<&TweetData> for Tweet {
   type Error = Error;

   #[expect(clippy::option_if_let_else, reason = "readability")]
   fn try_from(raw: &TweetData) -> Result<Self> {
      // Handle tombstones/unavailable tweets
      match raw.__typename.as_deref() {
         Some("TweetTombstone") => {
            let text = raw
               .tombstone
               .as_deref()
               .unwrap_or("Tweet is unavailable")
               .to_owned();
            return Ok(Self {
               available: false,
               tombstone: text,
               ..Default::default()
            });
         },
         Some("TweetUnavailable") => {
            let reason = raw.reason.as_deref().unwrap_or("unavailable").to_owned();
            return Ok(Self {
               available: false,
               tombstone: reason,
               ..Default::default()
            });
         },
         Some("TweetPreviewDisplay") => {
            return Ok(Self {
               available: false,
               tombstone: "You're unable to view this Tweet because it's only available to the \
                           Subscribers of the account owner."
                  .to_owned(),
               ..Default::default()
            });
         },
         Some("TweetWithVisibilityResults") => {
            if let Some(inner) = raw.tweet.as_deref() {
               return Self::try_from(inner);
            }
         },
         _ => {},
      }

      let default_legacy = TweetLegacy::default();
      let legacy = raw.legacy.as_ref().unwrap_or(&default_legacy);

      let id = legacy
         .id_str
         .as_deref()
         .or(raw.rest_id.as_deref())
         .and_then(|id_str| id_str.parse().ok())
         .unwrap_or(0_i64);

      if id == 0 {
         return Err(Error::TweetNotFound("No tweet ID found".into()));
      }

      let is_withheld = legacy.is_withheld();

      // Check for note_tweet (long tweets >280 chars) which provides the full
      // untruncated text
      let note_tweet = raw.note_tweet.as_ref();

      let mut text = note_tweet.map_or_else(
         || legacy.full_text().to_owned(),
         |nt| nt.text.clone().unwrap_or_default(),
      );

      // Parse user (check both user_results and user_result paths)
      let user = raw
         .core
         .as_ref()
         .and_then(|core| core.user_value())
         .and_then(|user_data| parse_user_object(user_data).ok())
         .unwrap_or_default();

      // Parse stats
      let stats = TweetStats {
         replies:  legacy.reply_count,
         retweets: legacy.retweet_count,
         likes:    legacy.favorite_count,
         quotes:   legacy.quote_count,
         views:    raw
            .views
            .as_ref()
            .and_then(|views| views.count.as_ref())
            .and_then(|count| count.parse().ok())
            .unwrap_or(0),
      };

      // Parse media from typed legacy fields (no side effects on text)
      let media = parse_media(legacy);
      let mut photos = media.photos;
      let video = media.video;
      let gif = media.gif;
      let media_attribution = media.attribution;

      // Strip media URLs from tweet text
      for url in &media.strip_urls {
         if !url.is_empty() && text.ends_with(url) {
            let new_len = text.len() - url.len();
            text.truncate(new_len);
            #[expect(
               clippy::assigning_clones,
               reason = "clone_into cannot borrow text mutably while trim_end borrows it"
            )]
            {
               text = text.trim_end().to_owned();
            }
            break;
         }
      }

      let time = legacy.parse_time();

      // Parse reply info (with GraphQL fallbacks)
      let mut reply_id = legacy.reply_id();

      // Fallback: reply_to_results.rest_id (newer GraphQL path)
      if reply_id == 0
         && let Some(ref rtr) = raw.reply_to_results
      {
         reply_id = rtr
            .rest_id
            .as_deref()
            .and_then(|id_str| id_str.parse().ok())
            .unwrap_or(0);
      }

      let mut reply: Vec<String> = legacy
         .in_reply_to_screen_name
         .as_ref()
         .map(|name| vec![name.clone()])
         .unwrap_or_default();

      // Fallback: reply_to_user_results for reply username (newer GraphQL path)
      if reply.is_empty()
         && let Some(screen_name) = raw.reply_to_screen_name.as_ref()
      {
         reply = vec![screen_name.clone()];
      }

      // Parse thread info
      let has_thread = legacy.self_thread.is_some();
      let thread_id = legacy.thread_id(id);

      // Parse quote tweet (check both paths)
      let quote = {
         let qsr = raw
            .quoted_status_result
            .as_ref()
            .or(raw.quoted_post_results.as_ref());
         match qsr {
            Some(result) => {
               result.result.as_deref().map_or_else(
                  || {
                     // quotedPostResults present but no result -- create stub with just the ID
                     let quoted_id = legacy
                        .quoted_status_id_str
                        .as_deref()
                        .and_then(|id_str| id_str.parse().ok())
                        .unwrap_or(0);
                     (quoted_id != 0).then(|| {
                        Box::new(Self {
                           id: quoted_id,
                           ..Default::default()
                        })
                     })
                  },
                  |quote_data| parse_tweet_object(quote_data).ok().map(Box::new),
               )
            },
            // Fallback: is_quote_status + quoted_status_id_str
            None => {
               if legacy.is_quote_status.unwrap_or(false) {
                  let quoted_id = legacy
                     .quoted_status_id_str
                     .as_deref()
                     .and_then(|id_str| id_str.parse().ok())
                     .unwrap_or(0);
                  (quoted_id != 0).then(|| {
                     Box::new(Self {
                        id: quoted_id,
                        ..Default::default()
                     })
                  })
               } else {
                  None
               }
            },
         }
      };

      // Parse retweet (check legacy path and newer repostedStatusResults)
      let retweet = legacy
         .retweeted_status_result
         .as_ref()
         .or(raw.reposted_status_results.as_ref())
         .and_then(|nested| nested.result.as_deref())
         .and_then(|rt_data| parse_tweet_object(rt_data).ok())
         .map(Box::new);

      // Parse card, then expand card URL via tweet entities
      let mut card = raw
         .card
         .as_ref()
         .and_then(|card_data| Card::try_from(card_data).ok());
      if let Some(ref mut card_ref) = card
         && !card_ref.url.is_empty()
         && let Some(expanded) = legacy.expand_card_url(&card_ref.url)
      {
         card_ref.url = expanded;
      }

      // Parse poll
      let poll = card
         .as_ref()
         .filter(|card_ref| matches!(card_ref.kind, CardKind::Unknown))
         .and(raw.card.as_ref())
         .and_then(|card_data| Poll::try_from(card_data).ok());

      let location = legacy.location().to_owned();

      // Parse entities for text expansion
      // Use note_tweet entity_set if available (for long tweets), otherwise use
      // legacy entities
      let mut entities = note_tweet.map_or_else(
         || {
            legacy
               .entities
               .as_ref()
               .map_or_else(Vec::new, parse_entities_from)
         },
         |nt| {
            nt.entity_set
               .as_ref()
               .map_or_else(Vec::new, parse_entities_from)
         },
      );

      // Apply display_text_range to strip leading @reply mentions
      #[expect(
         clippy::cast_possible_truncation,
         reason = "display_start fits in usize"
      )]
      let display_start = legacy
         .display_text_range
         .as_ref()
         .and_then(|range| range.first().copied())
         .unwrap_or(0) as usize;

      // Collect leading @mention entities into the reply field so the "Replying to"
      // line shows all recipients, not just in_reply_to_screen_name.
      //
      // For note_tweets: the note_tweet entity_set often doesn't include
      // user_mentions, so fall back to legacy entities for mention collection.
      // The note_tweet text is already stripped of the @mention prefix.
      if reply_id != 0
         && note_tweet.is_some()
         && let Some(ref ent) = legacy.entities
      {
         for mention in &ent.user_mentions {
            let screen_name = mention.screen_name.as_deref().unwrap_or_default();
            let (start, _) = indices(&mention.indices);
            if start < display_start
               && !screen_name.is_empty()
               && !reply
                  .iter()
                  .any(|existing| existing.eq_ignore_ascii_case(screen_name))
            {
               reply.push(screen_name.to_owned());
            }
         }
      }

      if display_start > 0 && note_tweet.is_none() {
         for entity in &entities {
            if entity.indices.0 < display_start && entity.kind == EntityKind::Mention {
               // url is "/{screen_name}", strip the leading "/"
               let screen_name = entity.url.trim_start_matches('/');
               if !screen_name.is_empty()
                  && !reply
                     .iter()
                     .any(|existing| existing.eq_ignore_ascii_case(screen_name))
               {
                  reply.push(screen_name.to_owned());
               }
            }
         }

         // Trim the text: skip the first display_start characters (Unicode codepoints)
         let char_byte_offset = text
            .char_indices()
            .nth(display_start)
            .map_or(0, |(byte_idx, _)| byte_idx);
         if char_byte_offset > 0 {
            #[expect(
               clippy::assigning_clones,
               reason = "clone_into cannot borrow text mutably while slicing borrows it"
            )]
            {
               text = text[char_byte_offset..].trim_start().to_owned();
            }
         }

         // Adjust entity indices and filter out entities before the display range
         entities.retain(|ent| ent.indices.0 >= display_start);
         for entity in &mut entities {
            entity.indices.0 -= display_start;
            entity.indices.1 -= display_start;
         }
      }

      // Parse community note (Birdwatch pivot)
      let note = raw
         .birdwatch_pivot
         .as_ref()
         .and_then(super::super::schema::BirdwatchPivot::to_note);

      // Parse content disclosure labels
      let (paid_promotion, ai_generated) =
         raw.content_disclosure
            .as_ref()
            .map_or((false, false), |cd| {
               let paid = cd
                  .advertising_disclosure
                  .as_ref()
                  .is_some_and(|ad| ad.is_paid_promotion);
               let ai = cd
                  .ai_generated_disclosure
                  .as_ref()
                  .is_some_and(|ag| ag.has_ai_generated_media);
               (paid, ai)
            });

      // Parse edit history IDs
      let history = raw
         .edit_control
         .as_ref()
         .and_then(|ec| ec.tweet_ids())
         .map(|ids| {
            ids.iter()
               .filter_map(|id_str| id_str.parse::<i64>().ok())
               .collect()
         })
         .unwrap_or_default();

      // Extract poll image into photos
      if let Some(ref poll) = poll
         && let Some(ref img_url) = poll.image
      {
         photos.push(Photo {
            url:      img_url.clone(),
            alt_text: String::new(),
         });
      }

      // Handle amplify card: sets tweet.video directly
      let (mut card, video) = if card
         .as_ref()
         .is_some_and(|card_ref| card_ref.kind == CardKind::Amplify)
      {
         let amplify_video = card.and_then(|card_ref| card_ref.video);
         (None, video.or(amplify_video))
      } else {
         (card, video)
      };

      // Strip " Learn more." from withheld text
      if is_withheld {
         #[expect(
            clippy::assigning_clones,
            reason = "clone_into cannot borrow text mutably while trim borrows it"
         )]
         {
            text = text.trim_end_matches(" Learn more.").to_owned();
         }
      }

      // Synthesize a rich card from inline article data when no card exists yet.
      // The TweetDetail response includes article metadata when
      // withArticleRichContentState is true. When we have a card, strip the
      // article URL from the text (the card is clickable). Otherwise, keep it
      // as a "View article" text link fallback.
      let has_article_card = card.is_none() && {
         if let Some(article) = raw
            .article
            .as_ref()
            .and_then(|wrapper| wrapper.article_results.as_ref())
            .and_then(|nr| nr.result.as_deref())
            && let Some(title) = article.title.as_deref().filter(|tt| !tt.is_empty())
         {
            let image = article
               .cover_media
               .as_ref()
               .and_then(|cm| cm.media_info.as_ref())
               .and_then(|mi| mi.original_img_url.clone())
               .unwrap_or_default();

            let description = article
               .content_state
               .as_ref()
               .and_then(|cs| {
                  cs.blocks
                     .iter()
                     .find(|blk| blk.block_type == "unstyled" && !blk.text.is_empty())
               })
               .map(|blk| {
                  let desc = &blk.text;
                  if desc.len() > 200 {
                     let mut end = 200;
                     while !desc.is_char_boundary(end) {
                        end -= 1;
                     }
                     format!("{}…", &desc[..end])
                  } else {
                     desc.clone()
                  }
               })
               .unwrap_or_default();

            card = Some(Card {
               kind: CardKind::Article,
               url: format!("/{}/article/{id}", user.username),
               title: title.to_owned(),
               image,
               text: description,
               dest: format!("Article · @{}", user.username),
               ..Card::default()
            });
            true
         } else {
            false
         }
      };

      // Rewrite article entity URLs: when a rich card was synthesized, strip
      // the trailing article link from both text and entities (the card is
      // clickable). Otherwise rewrite to a "View article" fallback link.
      entities.retain_mut(|entity| {
         if entity.kind != EntityKind::Url || !is_article_url(&entity.url) {
            return true;
         }
         if has_article_card {
            let start = text
               .char_indices()
               .nth(entity.indices.0)
               .map_or(text.len(), |(idx, _)| idx);
            text.truncate(start);
            #[expect(
               clippy::assigning_clones,
               reason = "clone_into cannot borrow text mutably while trim borrows it"
            )]
            {
               text = text.trim_end().to_owned();
            }
            false
         } else {
            entity.url = format!("/{}/article/{id}", user.username);
            "View article".clone_into(&mut entity.display);
            true
         }
      });

      let lang = legacy.lang.as_deref().unwrap_or_default().to_owned();

      Ok(Self {
         id,
         thread_id,
         reply_id,
         user,
         text: text.clone(),
         time,
         reply,
         pinned: false,
         has_thread,
         available: !is_withheld,
         tombstone: if is_withheld { text } else { String::new() },
         location,
         source: String::new(),
         stats,
         retweet,
         attribution: media_attribution,
         media_tags: Vec::new(),
         quote,
         card,
         poll,
         gif,
         video,
         photos,
         note,
         paid_promotion,
         ai_generated,
         history,
         entities,
         lang,
      })
   }
}

/// Check if a URL matches `http(s)://(x.com|twitter.com)/i/article/{id}`.
fn is_article_url(url: &str) -> bool {
   // Strip scheme first, then match domain
   let without_scheme = url
      .strip_prefix("https://")
      .or_else(|| url.strip_prefix("http://"));
   let Some(rest) = without_scheme else {
      return false;
   };
   let Some(path) = rest
      .strip_prefix("x.com")
      .or_else(|| rest.strip_prefix("www.x.com"))
      .or_else(|| rest.strip_prefix("mobile.x.com"))
      .or_else(|| rest.strip_prefix("twitter.com"))
      .or_else(|| rest.strip_prefix("www.twitter.com"))
      .or_else(|| rest.strip_prefix("mobile.twitter.com"))
   else {
      return false;
   };
   let Some(id) = path.strip_prefix("/i/article/") else {
      return false;
   };
   !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
}
