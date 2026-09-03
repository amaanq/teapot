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

impl TimelineKind {
   pub const fn as_str(self) -> &'static str {
      match self {
         Self::Tweets => "tweets",
         Self::Replies => "replies",
         Self::Media => "media",
         Self::Search => "search",
      }
   }
}

/// How X orders the replies under a post.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RankingMode {
   #[default]
   Relevance,
   Recency,
   Likes,
}

impl RankingMode {
   pub fn from_sort(sort: Option<&str>) -> Self {
      match sort {
         Some("recency") => Self::Recency,
         Some("likes") => Self::Likes,
         _ => Self::Relevance,
      }
   }

   pub const fn as_str(self) -> &'static str {
      match self {
         Self::Relevance => "Relevance",
         Self::Recency => "Recency",
         Self::Likes => "Likes",
      }
   }

   pub const fn sort_param(self) -> Option<&'static str> {
      match self {
         Self::Relevance => None,
         Self::Recency => Some("recency"),
         Self::Likes => Some("likes"),
      }
   }

   pub const fn label(self) -> &'static str {
      match self {
         Self::Relevance => "Relevant",
         Self::Recency => "Recent",
         Self::Likes => "Likes",
      }
   }
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
