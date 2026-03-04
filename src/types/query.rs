use std::fmt::Write as _;

use serde::{
   Deserialize,
   Serialize,
};

/// Valid Twitter search filters.
pub const VALID_FILTERS: &[&str] = &[
   "media",
   "images",
   "twimg",
   "videos",
   "native_video",
   "consumer_video",
   "pro_video",
   "spaces",
   "links",
   "news",
   "quote",
   "mentions",
   "replies",
   "retweets",
   "nativeretweets",
   "verified",
   "blue_verified",
   "safe",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueryKind {
   #[default]
   Posts,
   Replies,
   Media,
   Users,
   Tweets,
   UserList,
   Top,
}

/// Search query with filters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Query {
   #[serde(default)]
   pub kind:      QueryKind,
   #[serde(default)]
   pub text:      String,
   #[serde(default)]
   pub filters:   Vec<String>,
   #[serde(default)]
   pub includes:  Vec<String>,
   #[serde(default)]
   pub excludes:  Vec<String>,
   #[serde(default)]
   pub from_user: Vec<String>,
   #[serde(default)]
   pub since:     String,
   #[serde(default)]
   pub until:     String,
   #[serde(default)]
   pub min_likes: String,
   #[serde(default)]
   pub sep:       String,
}

impl Query {
   /// Build the search query string for Twitter API.
   pub fn build(&self) -> String {
      let mut param = String::new();

      // Add from:user with OR (from users come first)
      for (idx, user) in self.from_user.iter().enumerate() {
         let _ = write!(param, "from:{user} ");
         if idx < self.from_user.len() - 1 {
            param.push_str("OR ");
         }
      }

      // Add self_threads filter for from-user queries with posts/media kind
      if !self.from_user.is_empty() && matches!(self.kind, QueryKind::Posts | QueryKind::Media) {
         param.push_str("filter:self_threads OR -filter:replies ");
      }

      // Add include:nativeretweets unless explicitly excluded
      if !self.excludes.contains(&"nativeretweets".to_owned()) {
         param.push_str("include:nativeretweets ");
      }

      // Build filters
      let mut filters = Vec::new();
      for filter in &self.filters {
         filters.push(format!("filter:{filter}"));
      }
      for exclude in &self.excludes {
         if exclude == "nativeretweets" {
            continue;
         }
         filters.push(format!("-filter:{exclude}"));
      }
      for inc in &self.includes {
         filters.push(format!("include:{inc}"));
      }

      let sep = if self.sep.is_empty() { " " } else { &self.sep };
      let mut result = format!("{param}{}", filters.join(&format!(" {sep} ")))
         .trim()
         .to_owned();

      if !self.since.is_empty() {
         let _ = write!(result, " since:{}", self.since);
      }
      if !self.until.is_empty() {
         let _ = write!(result, " until:{}", self.until);
      }
      if !self.min_likes.is_empty() {
         let _ = write!(result, " min_faves:{}", self.min_likes);
      }

      // Add text last
      if !self.text.is_empty() {
         if result.is_empty() {
            result.clone_from(&self.text);
         } else {
            let _ = write!(result, " {}", self.text);
         }
      }

      result
   }

   /// Parse a query string into a [`Query`] struct.
   /// Extracts filters like "from:user", "filter:media", "since:2024-01-01"
   /// etc.
   pub fn parse(text: &str, kind: QueryKind) -> Self {
      let mut query = Self {
         kind,
         ..Default::default()
      };

      let mut remaining_text = Vec::new();

      for part in text.split_whitespace() {
         if let Some(value) = part.strip_prefix("from:") {
            // Handle comma-separated users
            for user in value.split(',') {
               let user = user.trim();
               if !user.is_empty() {
                  query.from_user.push(user.to_owned());
               }
            }
         } else if let Some(value) = part.strip_prefix("filter:") {
            if VALID_FILTERS.contains(&value) {
               query.filters.push(value.to_owned());
            } else {
               // Unknown filter, keep as text
               remaining_text.push(part.to_owned());
            }
         } else if let Some(value) = part.strip_prefix("-filter:") {
            if VALID_FILTERS.contains(&value) {
               query.excludes.push(value.to_owned());
            } else {
               remaining_text.push(part.to_owned());
            }
         } else if let Some(value) = part.strip_prefix("include:") {
            query.includes.push(value.to_owned());
         } else if let Some(value) = part.strip_prefix("exclude:") {
            query.excludes.push(value.to_owned());
         } else if let Some(value) = part.strip_prefix("since:") {
            // Validate date format (basic check)
            if is_valid_date(value) {
               value.clone_into(&mut query.since);
            } else {
               remaining_text.push(part.to_owned());
            }
         } else if let Some(value) = part.strip_prefix("until:") {
            if is_valid_date(value) {
               value.clone_into(&mut query.until);
            } else {
               remaining_text.push(part.to_owned());
            }
         } else if let Some(value) = part.strip_prefix("min_faves:") {
            if value.chars().all(|ch| ch.is_ascii_digit()) {
               value.clone_into(&mut query.min_likes);
            } else {
               remaining_text.push(part.to_owned());
            }
         } else if let Some(value) = part.strip_prefix("min_retweets:") {
            // Not directly supported, but keep for completeness
            if value.chars().all(|ch| ch.is_ascii_digit()) {
               remaining_text.push(part.to_owned());
            }
         } else if part.starts_with('-') && part.len() > 1 {
            // Negative word filter like "-spam"
            query.excludes.push(part[1..].to_string());
         } else {
            remaining_text.push(part.to_owned());
         }
      }

      query.text = remaining_text.join(" ");
      query
   }

   /// Generate URL parameters for preserving search state.
   pub fn to_url_params(&self) -> String {
      let mut params = Vec::new();

      if !self.text.is_empty() {
         params.push(format!("q={}", urlencoding::encode(&self.text)));
      }

      match self.kind {
         QueryKind::Posts => {},
         QueryKind::Replies => params.push("f=replies".to_owned()),
         QueryKind::Media => params.push("f=media".to_owned()),
         QueryKind::Users => params.push("f=users".to_owned()),
         QueryKind::Tweets => params.push("f=tweets".to_owned()),
         QueryKind::UserList => params.push("f=userlist".to_owned()),
         QueryKind::Top => params.push("f=top".to_owned()),
      }

      for filter in &self.filters {
         params.push(format!("f-{filter}=on"));
      }

      for exclude in &self.excludes {
         params.push(format!("e-{exclude}=on"));
      }

      for user in &self.from_user {
         params.push(format!("from={}", urlencoding::encode(user)));
      }

      if !self.since.is_empty() {
         params.push(format!("since={}", self.since));
      }

      if !self.until.is_empty() {
         params.push(format!("until={}", self.until));
      }

      if !self.min_likes.is_empty() {
         params.push(format!("min_faves={}", self.min_likes));
      }

      params.join("&")
   }

   /// Check if query has any active filters.
   pub const fn has_filters(&self) -> bool {
      !self.filters.is_empty()
         || !self.excludes.is_empty()
         || !self.from_user.is_empty()
         || !self.since.is_empty()
         || !self.until.is_empty()
         || !self.min_likes.is_empty()
   }
}

/// Basic date validation (YYYY-MM-DD format).
fn is_valid_date(date_str: &str) -> bool {
   if date_str.len() != 10 {
      return false;
   }

   let parts = date_str.split('-').collect::<Vec<_>>();
   if parts.len() != 3 {
      return false;
   }

   parts[0].len() == 4
      && parts[1].len() == 2
      && parts[2].len() == 2
      && parts
         .iter()
         .all(|part| part.chars().all(|ch| ch.is_ascii_digit()))
}

mod urlencoding {
   pub fn encode(input: &str) -> String {
      percent_encoding::utf8_percent_encode(input, percent_encoding::NON_ALPHANUMERIC).to_string()
   }
}
