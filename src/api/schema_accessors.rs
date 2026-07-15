use super::{
   BindingValues,
   CardData,
   EditControl,
   MediaItem,
   TweetCore,
   TweetLegacy,
   UserData,
   parse_twitter_time,
};

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
