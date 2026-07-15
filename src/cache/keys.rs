pub fn user(username: &str) -> String {
   format!("u:{}", username.to_lowercase())
}

pub fn profile(username: &str) -> String {
   format!("p:{}", username.to_lowercase())
}

pub fn timeline(username: &str, kind: &str) -> String {
   format!("tl:{}:{kind}", username.to_lowercase())
}

pub fn list(id: &str) -> String {
   format!("l:{id}")
}

pub fn list_members(id: &str) -> String {
   format!("lm:{id}")
}

pub fn conversation(id: &str) -> String {
   format!("conv:{id}")
}

pub fn tweet(id: &str) -> String {
   format!("tweet:{id}")
}

pub fn translation(id: i64, backend: &str) -> String {
   format!("translation:{id}:en:{backend}")
}

pub fn rss(key: &str) -> String {
   format!("rss:{key}")
}

/// User ID to username mapping.
pub fn user_id(id: &str) -> String {
   format!("uid:{id}")
}

pub fn rss_user(username: &str) -> String {
   rss(&format!("user:{}", username.to_lowercase()))
}

pub fn rss_replies(username: &str) -> String {
   rss(&format!("replies:{}", username.to_lowercase()))
}

pub fn rss_media(username: &str) -> String {
   rss(&format!("media:{}", username.to_lowercase()))
}

pub fn rss_search(query: &str) -> String {
   rss(&format!("search:{}", query.to_lowercase()))
}

pub fn rss_user_search(username: &str, query: &str) -> String {
   rss(&format!(
      "usersearch:{}:{}",
      username.to_lowercase(),
      query.to_lowercase()
   ))
}

pub fn rss_list(id: &str) -> String {
   rss(&format!("list:{id}"))
}

pub fn rss_list_slug(username: &str, slug: &str) -> String {
   rss(&format!("listslug:{}:{slug}", username.to_lowercase()))
}

pub fn rss_thread(tweet_id: &str) -> String {
   rss(&format!("thread:{tweet_id}"))
}
