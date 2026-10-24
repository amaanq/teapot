#![expect(
   clippy::same_name_method,
   clippy::shadow_same,
   clippy::shadow_unrelated,
   reason = "topcoat's #[page] and view! expansions"
)]

use std::collections::{
   HashMap,
   HashSet,
   VecDeque,
};

use tokio::task::JoinSet;
use topcoat::{
   context::Cx,
   router::{
      page,
      path_param,
   },
   view::{
      View,
      view,
   },
};

use crate::{
   api::budget,
   routes::helpers::{
      request_prefs,
      state,
   },
   types::article::{
      ArticleBlockType,
      ArticleEntityType,
   },
   utils::formatters,
   views::{
      document::document,
      notes::note,
   },
};

path_param!(id);

/// Serves `/i/article/{id}` using only the tweet ID.
#[page("/i/article/{id}")]
pub async fn show_note_by_id(cx: &Cx) -> topcoat::Result<impl View> {
   show_note_inner(cx).await
}

/// Serves `/{username}/article/{id}` with username context.
#[page("/{username}/article/{id}")]
pub async fn show_note(cx: &Cx) -> topcoat::Result<impl View> {
   show_note_inner(cx).await
}

async fn show_note_inner(cx: &Cx) -> topcoat::Result<impl View> {
   let state = state(cx);
   let prefs = request_prefs(cx);
   let (_tweet, article) = state.api.get_article_tweet(path_param::<Id>(cx)).await?;

   // Fetch embedded tweets concurrently with a small bound so a large article
   // cannot consume the entire upstream session pool.
   let mut seen = HashSet::new();
   let mut tweet_ids = article
      .entities
      .iter()
      .filter(|entity| entity.entity_type == ArticleEntityType::Tweet)
      .filter(|entity| !entity.tweet_id.is_empty())
      .filter(|entity| seen.insert(entity.tweet_id.clone()))
      .map(|entity| entity.tweet_id.clone())
      .collect::<VecDeque<_>>();

   let mut tweets = HashMap::new();
   let mut tasks = JoinSet::new();
   for _ in 0..8 {
      let Some(tweet_id) = tweet_ids.pop_front() else {
         break;
      };
      let api = state.api.clone();
      tasks.spawn(budget::scoped(
         async move { api.get_tweet(&tweet_id).await },
      ));
   }
   while let Some(joined) = tasks.join_next().await {
      if let Ok(Ok(tweet)) = joined {
         tweets.insert(tweet.id, tweet);
      }
      if let Some(tweet_id) = tweet_ids.pop_front() {
         let api = state.api.clone();
         tasks.spawn(budget::scoped(
            async move { api.get_tweet(&tweet_id).await },
         ));
      }
   }

   let description = article
      .paragraphs
      .iter()
      .find(|para| para.base_type == ArticleBlockType::Unstyled && !para.text.is_empty())
      .map_or_default(|para| {
         if para.text.len() > 200 {
            let mut end = 200;
            while !para.text.is_char_boundary(end) {
               end -= 1;
            }
            format!("{}…", &para.text[..end])
         } else {
            para.text.clone()
         }
      });

   // Proxy the og:image cover through the media pipeline
   let og_image = if article.cover_image.is_empty() {
      String::new()
   } else {
      let proxied = formatters::get_pic_url(&article.cover_image, state.config.config.base64_media);
      format!("{}{proxied}", state.config.url_prefix())
   };

   // twitter:card = summary_large_image for big cover preview in Discord/etc
   let has_cover = !article.cover_image.is_empty();
   let head = view! { cx =>
      if has_cover {
         <meta name="twitter:card" content="summary_large_image">
      }
   };

   Ok(view! { cx =>
      document(
         title: &article.title,
         prefs: &prefs,
         description: &description,
         og_image: &og_image,
         head: Some(head.into()),
         note(article: &article, tweets: &tweets, prefs: &prefs)
      )
   })
}
