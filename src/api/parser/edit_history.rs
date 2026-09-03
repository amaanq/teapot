use super::tweet::parse_tweet_object;
use crate::{
   api::{
      schema::InstructionType,
      schema_responses::EditHistoryData,
   },
   error::{
      Error,
      Result,
   },
   types::timeline::EditHistory,
};

/// Parse a `TweetEditHistory` response.
#[expect(
   clippy::module_name_repetitions,
   reason = "clear naming for public API"
)]
pub fn parse_edit_history(data: &EditHistoryData) -> Result<EditHistory> {
   let raw_instructions = data.instructions();

   let mut result = EditHistory::default();

   for instruction in raw_instructions {
      if instruction.instruction_type != Some(InstructionType::TimelineAddEntries) {
         continue;
      }

      for entry in instruction.entries.as_deref().unwrap_or(&[]) {
         let entry_id = entry.entry_id_str();

         if entry_id == "latestTweet" {
            // Latest version is in items[0]
            if let Some(tweet) = entry
               .items()
               .first()
               .and_then(|item| item.tweet_result())
               .map(parse_tweet_object)
               .transpose()?
            {
               result.latest = tweet;
            }
         } else if entry_id == "staleTweets" {
            // Previous versions
            for item in entry.items() {
               if let Some(tweet) = item
                  .tweet_result()
                  .and_then(|tw_result| parse_tweet_object(tw_result).ok())
               {
                  result.history.push(tweet);
               }
            }
         }
      }
   }

   if result.latest.id == 0 {
      return Err(Error::TweetNotFound("No edit history found".to_owned()));
   }

   Ok(result)
}
