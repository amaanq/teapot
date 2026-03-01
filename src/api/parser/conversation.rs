use super::tweet::parse_tweet_object;
use crate::{
   api::schema::{
      ConversationData,
      InstructionType,
   },
   error::{
      Error,
      Result,
   },
   types::{
      Chain,
      Conversation,
      PaginatedResult,
      Tweet,
   },
};

/// Parse a conversation from `TweetDetail` response.
pub fn parse_conversation(
   data: &ConversationData,
   tweet_id: &str,
   has_cursor: bool,
) -> Result<Conversation> {
   // Single tweet result path
   if let Some(tweet_data) = data
      .tweet_result
      .as_ref()
      .and_then(|nested| nested.result.as_deref())
      && tweet_data.__typename.is_some()
   {
      let tweet = parse_tweet_object(tweet_data)?;
      return Ok(Conversation {
         tweet,
         ..Default::default()
      });
   }

   // Instructions path
   let raw_instructions = data
      .threaded_conversation_with_injections_v2
      .as_ref()
      .map(|conv| conv.instructions.as_slice())
      .filter(|instr| !instr.is_empty())
      .ok_or_else(|| Error::TweetNotFound("No instructions array".to_owned()))?;

   let mut main_tweet = Option::<Tweet>::None;
   let mut before = Chain::default();
   let mut after = Chain::default();
   let mut replies = PaginatedResult::<Chain>::default();

   // Parse tweet_id for matching
   let target_id = tweet_id.parse().unwrap_or(0);

   for instruction in raw_instructions {
      if instruction.instruction_type != Some(InstructionType::TimelineAddEntries) {
         continue;
      }

      for entry in instruction.entries.as_deref().unwrap_or_default() {
         let entry_id = entry.entry_id_str();

         if entry_id.starts_with("tweet-") {
            // Single tweet entry -- match by ID to determine main vs before
            if let Some(tweet) = entry
               .tweet_result()
               .and_then(|result| parse_tweet_object(result).ok())
            {
               if tweet.id == target_id {
                  main_tweet = Some(tweet);
               } else {
                  before.content.push(tweet);
               }
            }
         } else if entry_id.starts_with("tombstone") {
            // Tombstone entry -- unavailable tweet in conversation
            let tombstone_id = entry_id
               .split('-')
               .next_back()
               .and_then(|id_str| id_str.parse().ok())
               .unwrap_or(0);
            let tweet = Tweet {
               id: tombstone_id,
               available: false,
               tombstone: "This tweet is unavailable".to_owned(),
               ..Default::default()
            };
            if tombstone_id == target_id {
               main_tweet = Some(tweet);
            } else {
               before.content.push(tweet);
            }
         } else if entry_id.starts_with("conversationthread-") {
            // Thread/replies -- check for self-thread (after) vs other replies
            let mut chain = Chain::default();
            let mut is_self_thread = false;

            for item in entry.items() {
               // Check for "Show more" cursor in thread
               if item.entry_id_str().contains("cursor-showmore") {
                  let cursor = item.cursor_value().unwrap_or_default();
                  chain.has_more = true;
                  chain.cursor = Some(cursor.to_owned());
                  continue;
               }

               if let Some(tweet_result) = item.tweet_result()
                  && let Ok(tweet) = parse_tweet_object(tweet_result)
               {
                  chain.content.push(tweet);
               }

               // Check if this is a self-thread
               if item.display_type() == Some("SelfThread") {
                  is_self_thread = true;
               }
            }

            if !chain.content.is_empty() {
               if is_self_thread {
                  after = chain;
               } else {
                  replies.content.push(chain);
               }
            }
         } else if entry_id.starts_with("cursor-") {
            // Pagination cursor
            if let Some(cursor_value) = entry.cursor_value() {
               if entry_id.contains("bottom") {
                  replies.bottom = Some(cursor_value.to_owned());
               } else if entry_id.contains("top") {
                  replies.top = Some(cursor_value.to_owned());
               }
            }
         }
      }
   }

   // Paginated requests (with cursor) don't include the main tweet in the
   // response — Twitter only returns the next batch of replies.
   let tweet = if has_cursor {
      main_tweet.unwrap_or_default()
   } else {
      main_tweet
         .ok_or_else(|| Error::TweetNotFound("Main tweet not found in conversation".to_owned()))?
   };

   Ok(Conversation {
      tweet,
      before,
      after,
      replies,
   })
}
