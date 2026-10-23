use serde::{
   Deserialize,
   Serialize,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerifiedType {
   #[default]
   None,
   Blue,
   Business,
   Government,
}

#[expect(
   clippy::struct_field_names,
   reason = "user_pic matches the API field name"
)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct User {
   pub id:                String,
   pub username:          String,
   pub fullname:          String,
   pub location:          String,
   pub account_based_in:  String,
   pub connection_source: String,
   pub location_accurate: Option<bool>,
   pub website:           String,
   pub bio:               String,
   pub user_pic:          String,
   pub banner:            String,
   pub pinned_tweet:      i64,
   pub following:         i64,
   pub followers:         i64,
   pub tweets:            i64,
   pub likes:             i64,
   pub media:             i64,
   pub verified_type:     VerifiedType,
   pub protected:         bool,
   pub suspended:         bool,
   #[serde(with = "jiff::fmt::serde::timestamp::second::optional")]
   pub join_date:         Option<jiff::Timestamp>,
}

/// X's "About this account" record.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountContext {
   pub account_based_in:  String,
   pub connection_source: String,
   pub location_accurate: Option<bool>,
}

impl AccountContext {
   /// Whether X returned anything worth showing.
   pub const fn is_empty(&self) -> bool {
      self.account_based_in.is_empty() && self.connection_source.is_empty()
   }
}

impl User {
   pub const fn is_empty(&self) -> bool {
      self.id.is_empty()
   }
}
