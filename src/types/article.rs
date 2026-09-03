#![expect(
   clippy::module_name_repetitions,
   reason = "the module is the namespace and the prefix names the domain"
)]
use std::collections::HashMap;

use serde::{
   Deserialize,
   Serialize,
};

use super::user::User;

/// A Twitter Article (long-form Notes).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Article {
   pub title:       String,
   pub cover_image: String,
   pub user:        User,
   #[serde(with = "time::serde::timestamp::option")]
   pub time:        Option<time::OffsetDateTime>,
   pub paragraphs:  Vec<ArticleParagraph>,
   pub entities:    Vec<ArticleEntity>,
   pub media:       HashMap<String, ArticleMedia>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArticleParagraph {
   pub text:                String,
   pub base_type:           ArticleBlockType,
   pub inline_style_ranges: Vec<ArticleStyleRange>,
   pub entity_ranges:       Vec<ArticleEntityRange>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArticleBlockType {
   Blockquote,
   CodeBlock,
   HeaderOne,
   HeaderTwo,
   HeaderThree,
   OrderedListItem,
   UnorderedListItem,
   #[default]
   Unstyled,
   Atomic,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArticleStyleRange {
   pub offset: usize,
   pub length: usize,
   pub style:  ArticleStyle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArticleStyle {
   Bold,
   Italic,
   Strikethrough,
   #[serde(other)]
   #[default]
   Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArticleEntityRange {
   pub offset: usize,
   pub length: usize,
   pub key:    usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArticleEntity {
   pub entity_type: ArticleEntityType,
   pub url:         String,
   pub media_ids:   Vec<String>,
   pub tweet_id:    String,
   pub twemoji:     String,
   pub markdown:    String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArticleEntityType {
   Link,
   Markdown,
   Media,
   Tweet,
   Twemoji,
   Divider,
   #[serde(other)]
   #[default]
   Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArticleMedia {
   pub media_type: ArticleMediaType,
   pub url:        String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArticleMediaType {
   ApiImage,
   ApiGif,
   #[serde(other)]
   #[default]
   Unknown,
}
