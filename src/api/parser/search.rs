#![expect(
   clippy::module_name_repetitions,
   reason = "the module is the namespace and the prefix names the domain"
)]
use super::user::parse_user_object;
use crate::{
   api::{
      schema::{
         InstructionType,
         ListData,
      },
      schema_responses::{
         ListMembersData,
         RetweetersData,
         SearchTimelineData,
      },
   },
   types::{
      query::Query,
      timeline::{
         List,
         PaginatedResult,
      },
      user::User,
   },
};

/// Parse user search results.
pub fn parse_user_search(data: &SearchTimelineData) -> PaginatedResult<User> {
   let raw_instructions = data.instructions();

   let mut users = Vec::new();
   let mut bottom_cursor = None;

   for instruction in raw_instructions {
      if instruction.instruction_type != Some(InstructionType::TimelineAddEntries) {
         continue;
      }

      for entry in instruction.entries.as_deref().unwrap_or_default() {
         let entry_id = entry.entry_id_str();

         if entry_id.starts_with("user-") {
            if let Some(user_result) = entry.user_result()
               && let Ok(user) = parse_user_object(user_result)
            {
               users.push(user);
            }
         } else if entry_id.starts_with("cursor-bottom-") {
            bottom_cursor = entry.cursor_value().map(str::to_owned);
         }
      }
   }

   PaginatedResult {
      content:   users,
      top:       None,
      bottom:    bottom_cursor,
      beginning: false,
      query:     Query::default(),
   }
}

/// Parse a list from typed `ListData`.
pub fn parse_list(raw: &ListData) -> List {
   let id = raw
      .id_str
      .as_deref()
      .or(raw.rest_id.as_deref())
      .unwrap_or_default()
      .to_owned();
   let name = raw.name.clone().unwrap_or_default();
   let description = raw.description.clone().unwrap_or_default();
   let members = raw.member_count;

   // Extract user info from nested user_results
   let (user_id, username) = raw
      .user_results
      .as_ref()
      .and_then(|nr| nr.result.as_ref())
      .and_then(|user_data| parse_user_object(user_data).ok())
      .map(|user| (user.id, user.username))
      .unwrap_or_default();

   let banner = raw.banner_url.as_deref().unwrap_or_default().to_owned();

   List {
      id,
      name,
      user_id,
      username,
      description,
      members,
      banner,
   }
}

/// Parse list members from API response.
pub fn parse_list_members(data: &ListMembersData) -> PaginatedResult<User> {
   let raw_instructions = data.instructions();

   let mut users = Vec::new();
   let mut top_cursor = None;
   let mut bottom_cursor = None;

   for instruction in raw_instructions {
      if instruction.instruction_type != Some(InstructionType::TimelineAddEntries) {
         continue;
      }

      for entry in instruction.entries.as_deref().unwrap_or_default() {
         let entry_id = entry.entry_id_str();

         if entry_id.starts_with("user-") {
            if let Some(user_result) = entry.user_result()
               && let Ok(user) = parse_user_object(user_result)
            {
               users.push(user);
            }
         } else if entry_id.starts_with("cursor-bottom-") {
            bottom_cursor = entry.cursor_value().map(str::to_owned);
         } else if entry_id.starts_with("cursor-top-") {
            top_cursor = entry.cursor_value().map(str::to_owned);
         }
      }
   }

   PaginatedResult {
      content:   users,
      top:       top_cursor,
      bottom:    bottom_cursor,
      beginning: false,
      query:     Query::default(),
   }
}

/// Parse retweeters from API response (same structure as list members).
pub fn parse_retweeters(data: &RetweetersData) -> PaginatedResult<User> {
   let raw_instructions = data.instructions();

   let mut users = Vec::new();
   let mut top_cursor = None;
   let mut bottom_cursor = None;

   for instruction in raw_instructions {
      if instruction.instruction_type != Some(InstructionType::TimelineAddEntries) {
         continue;
      }

      for entry in instruction.entries.as_deref().unwrap_or_default() {
         let entry_id = entry.entry_id_str();

         if entry_id.starts_with("user-") {
            if let Some(user_result) = entry.user_result()
               && let Ok(user) = parse_user_object(user_result)
            {
               users.push(user);
            }
         } else if entry_id.starts_with("cursor-bottom-") {
            bottom_cursor = entry.cursor_value().map(str::to_owned);
         } else if entry_id.starts_with("cursor-top-") {
            top_cursor = entry.cursor_value().map(str::to_owned);
         }
      }
   }

   PaginatedResult {
      content:   users,
      top:       top_cursor,
      bottom:    bottom_cursor,
      beginning: false,
      query:     Query::default(),
   }
}
