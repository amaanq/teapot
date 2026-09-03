#![expect(
   clippy::module_name_repetitions,
   reason = "the module is the namespace and the prefix names the domain"
)]
use super::{
   query::Query,
   tweet::{
      PhotoRail,
      Tweet,
   },
   user::User,
};

pub type Tweets = Vec<Tweet>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimelineKind {
   #[default]
   Tweets,
   Replies,
   Media,
   Search,
}

/// Generic paginated result.
#[derive(Debug, Clone, Default)]
pub struct PaginatedResult<T> {
   pub content:   Vec<T>,
   pub top:       Option<String>,
   pub bottom:    Option<String>,
   pub beginning: bool,
   pub query:     Query,
}

/// A chain of tweets (for conversation threads).
#[derive(Debug, Clone, Default)]
pub struct Chain {
   pub content:  Tweets,
   pub has_more: bool,
   pub cursor:   Option<String>,
}

impl Chain {
   pub fn contains(&self, tweet: &Tweet) -> bool {
      self.content.iter().any(|entry| entry.id == tweet.id)
   }
}

/// A conversation view (tweet + context + replies).
#[derive(Debug, Clone, Default)]
pub struct Conversation {
   pub tweet:   Tweet,
   pub before:  Chain,
   pub after:   Chain,
   pub replies: PaginatedResult<Chain>,
}

/// User timeline.
pub type Timeline = PaginatedResult<Tweets>;

/// User profile with tweets and photo rail.
#[derive(Debug, Clone, Default)]
pub struct Profile {
   pub user:       User,
   pub photo_rail: PhotoRail,
   pub pinned:     Option<Tweet>,
   pub tweets:     Timeline,
}

/// Edit history for a tweet.
#[derive(Debug, Clone, Default)]
pub struct EditHistory {
   pub latest:  Tweet,
   pub history: Tweets,
}

/// Twitter list.
#[derive(Debug, Clone, Default)]
pub struct List {
   pub id:          String,
   pub name:        String,
   pub user_id:     String,
   pub username:    String,
   pub description: String,
   pub members:     i32,
   pub banner:      String,
}
