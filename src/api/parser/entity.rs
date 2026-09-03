use crate::{
   api::schema::{
      Entities,
      indices,
   },
   types::tweet::{
      Entity,
      EntityKind,
   },
};

/// Parse entities from a typed `Entities` struct.
///
/// Stores only raw API data -- display formatting (`short_url`, hashtag
/// display) is deferred to `entity_expander::expand_entities()`.
pub fn parse_entities_from(raw: &Entities) -> Vec<Entity> {
   let mut entities = Vec::new();

   for url_ent in &raw.urls {
      let (start, end) = indices(&url_ent.indices);
      let expanded = url_ent.expanded_url.as_deref().unwrap_or_default();
      entities.push(Entity {
         indices: (start, end),
         kind: EntityKind::Url,
         url: expanded.to_owned(),
         ..Default::default()
      });
   }

   // Hashtag/symbol display and URLs are computed by the expander from the
   // original text -- only indices and kind are needed.
   for hashtag in &raw.hashtags {
      let (start, end) = indices(&hashtag.indices);
      entities.push(Entity {
         indices: (start, end),
         kind: EntityKind::Hashtag,
         ..Default::default()
      });
   }

   for mention in &raw.user_mentions {
      let (start, end) = indices(&mention.indices);
      let screen_name = mention.screen_name.as_deref().unwrap_or_default();
      let name = mention.name.as_deref().unwrap_or(screen_name);
      entities.push(Entity {
         indices: (start, end),
         kind:    EntityKind::Mention,
         url:     format!("/{screen_name}"),
         display: name.to_owned(),
      });
   }

   for symbol in &raw.symbols {
      let (start, end) = indices(&symbol.indices);
      entities.push(Entity {
         indices: (start, end),
         kind: EntityKind::Symbol,
         ..Default::default()
      });
   }

   entities.sort_by_key(|ent| ent.indices.0);
   entities
}
