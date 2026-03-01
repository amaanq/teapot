use super::tweet::parse_tweet_object;
use crate::{
   api::schema::{
      Entry,
      Instruction,
      InstructionType,
      ListTimelineData,
      SearchTimelineData,
      UserTimelineData,
   },
   error::{
      Error,
      Result,
   },
   types::{
      Query,
      Timeline,
      Tweets,
   },
};

/// Parse a user timeline (tweets, media, tweets-and-replies).
pub fn parse_timeline(data: &UserTimelineData) -> Result<Timeline> {
   let instructions = data
      .user
      .as_ref()
      .or(data.user_result.as_ref())
      .and_then(|nested| nested.result.as_ref())
      .map(super::super::schema::TimelineResultData::instructions)
      .filter(|instr| !instr.is_empty())
      .ok_or_else(|| Error::Internal("Timeline instructions not found".into()))?;

   parse_timeline_instructions(instructions)
}

/// Parse a list timeline.
pub fn parse_list_timeline(data: &ListTimelineData) -> Result<Timeline> {
   let instructions = data.instructions();
   if instructions.is_empty() {
      return Err(Error::Internal("Timeline instructions not found".into()));
   }
   parse_timeline_instructions(instructions)
}

fn parse_timeline_instructions(raw_instructions: &[Instruction]) -> Result<Timeline> {
   let mut tweets: Vec<Tweets> = Vec::new();
   let mut top_cursor = None;
   let mut bottom_cursor = None;

   for instruction in raw_instructions {
      // Handle moduleItems (grid modules used in media timelines)
      if let Some(ref module_items) = instruction.module_items {
         let mut module_tweets = Vec::new();
         for item in module_items {
            if let Some(tweet_result) = item.tweet_result()
               && let Ok(tweet) = parse_tweet_object(tweet_result)
            {
               module_tweets.push(tweet);
            }
         }
         if !module_tweets.is_empty() {
            tweets.push(module_tweets);
         }
         continue;
      }

      match instruction.instruction_type.unwrap_or_default() {
         InstructionType::TimelineAddEntries => {
            for entry in instruction.entries.as_deref().unwrap_or_default() {
               let entry_id = entry.entry_id_str();

               tracing::trace!("timeline entry: {entry_id}");
               if entry_id.contains("promoted") {
                  continue;
               }
               if entry_id.starts_with("tweet-")
                  || entry_id.contains("-conversation-")
                  || entry_id.starts_with("profile-grid-")
               {
                  let entry_tweets = parse_timeline_entry(entry);
                  if !entry_tweets.is_empty() {
                     tweets.push(entry_tweets);
                  }
               } else if entry_id.starts_with("cursor-bottom-") {
                  bottom_cursor = entry.cursor_value().map(str::to_owned);
               } else if entry_id.starts_with("cursor-top-") {
                  top_cursor = entry.cursor_value().map(str::to_owned);
               }
            }
         },
         InstructionType::TimelinePinEntry => {
            if let Some(mut tweet) = instruction
               .entry
               .as_ref()
               .and_then(|entry| entry.tweet_result())
               .map(parse_tweet_object)
               .transpose()?
            {
               tweet.pinned = true;
               tweets.insert(0, vec![tweet]);
            }
         },
         _ => {},
      }
   }

   Ok(Timeline {
      content:   tweets,
      top:       top_cursor,
      bottom:    bottom_cursor,
      beginning: false,
      query:     Query::default(),
   })
}

/// Parse tweets from a timeline entry.
fn parse_timeline_entry(entry: &Entry) -> Tweets {
   let mut tweets = Vec::new();

   // Single tweet entry
   if let Some(tweet_result) = entry.tweet_result() {
      if let Ok(tweet) = parse_tweet_object(tweet_result) {
         tweets.push(tweet);
      }
      return tweets;
   }

   // Conversation module (thread)
   for item in entry.items() {
      if let Some(tweet_result) = item.tweet_result()
         && let Ok(tweet) = parse_tweet_object(tweet_result)
      {
         tweets.push(tweet);
      }
   }

   tweets
}

/// Parse search results.
pub fn parse_search_timeline(data: &SearchTimelineData) -> Timeline {
   let raw_instructions = data.instructions();

   let mut tweets: Vec<Tweets> = Vec::new();
   let mut bottom_cursor = None;

   for instruction in raw_instructions {
      match instruction.instruction_type.unwrap_or_default() {
         InstructionType::TimelineAddEntries => {
            for entry in instruction.entries.as_deref().unwrap_or_default() {
               let entry_id = entry.entry_id_str();

               if entry_id.contains("promoted") {
                  continue;
               }
               if entry_id.starts_with("tweet-") {
                  if let Some(tweet_result) = entry.tweet_result()
                     && let Ok(tweet) = parse_tweet_object(tweet_result)
                  {
                     tweets.push(vec![tweet]);
                  }
               } else if entry_id.starts_with("cursor-bottom-") {
                  bottom_cursor = entry.cursor_value().map(str::to_owned);
               }
            }
         },
         // TimelineReplaceEntry: replace bottom cursor
         InstructionType::TimelineReplaceEntry => {
            if let Some(cursor) = instruction
               .entry_id_to_replace
               .as_deref()
               .filter(|id| id.starts_with("cursor-bottom"))
               .and(instruction.entry.as_ref())
               .and_then(|entry| entry.cursor_value())
            {
               bottom_cursor = Some(cursor.to_owned());
            }
         },
         _ => {},
      }
   }

   Timeline {
      content:   tweets,
      top:       None,
      bottom:    bottom_cursor,
      beginning: false,
      query:     Query::default(),
   }
}
