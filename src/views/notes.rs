#![expect(
   clippy::same_name_method,
   clippy::shadow_same,
   clippy::shadow_unrelated,
   reason = "topcoat's #[component] and view! expansions"
)]

use std::{
   collections::HashMap,
   fmt::Write as _,
};

use topcoat::{
   Result,
   context::Cx,
   view::{
      View,
      component,
      view,
   },
};

use crate::{
   types::{
      article::{
         Article,
         ArticleBlockType,
         ArticleEntityType,
         ArticleMediaType,
         ArticleParagraph,
         ArticleStyle,
      },
      prefs::Prefs,
      tweet::Tweet,
   },
   utils::{
      entity_expander::html_escape,
      formatters,
   },
   views::{
      renderutils::{
         config,
         proxied_image,
         trusted,
      },
      tweet::tweet_item,
   },
};

/// A run of paragraphs rendered under one element.
enum Block<'a> {
   List {
      ordered: bool,
      items:   Vec<&'a ArticleParagraph>,
   },
   Code(Vec<&'a str>),
   Single(&'a ArticleParagraph),
}

/// Group list items into one list per type and consecutive code-block lines
/// into one `<pre>`.
fn blocks(paragraphs: &[ArticleParagraph]) -> Vec<Block<'_>> {
   let mut blocks = Vec::<Block<'_>>::new();
   for para in paragraphs {
      let ordered = match para.base_type {
         ArticleBlockType::OrderedListItem => Some(true),
         ArticleBlockType::UnorderedListItem => Some(false),
         _ => None,
      };

      match (blocks.last_mut(), ordered, para.base_type) {
         (Some(&mut Block::Code(ref mut lines)), _, ArticleBlockType::CodeBlock) => {
            lines.push(&para.text);
         },
         (_, _, ArticleBlockType::CodeBlock) => blocks.push(Block::Code(vec![&para.text])),
         (
            Some(&mut Block::List {
               ordered: last_ordered,
               ref mut items,
            }),
            Some(this_ordered),
            _,
         ) if last_ordered == this_ordered => items.push(para),
         (_, Some(ordered), _) => {
            blocks.push(Block::List {
               ordered,
               items: vec![para],
            });
         },
         _ => blocks.push(Block::Single(para)),
      }
   }
   blocks
}

/// Render a Twitter Article (Notes) page.
#[component]
pub async fn note<'a>(
   cx: &Cx,
   article: &'a Article,
   tweets: &'a HashMap<i64, Tweet>,
   prefs: &'a Prefs,
) -> Result<impl View> {
   let config = config(cx);
   let cover = (!article.cover_image.is_empty())
      .then(|| formatters::get_pic_url(&article.cover_image, config.config.base64_media));
   let profile = format!("/{}", article.user.username);
   let avatar = formatters::get_user_pic(&article.user.user_pic, "_mini");

   Ok(view! {
      <div class="note">
         if let Some(cover) = cover {
            <img class="cover" src=(cover) alt="" loading="lazy">
         }
         <article>
            <h1>(&article.title)</h1>
            <div class="author">
               proxied_image(
                  url: &avatar,
                  class: "avatar round mini",
               )
               " "
               <a href=(&profile) class="fullname">(&article.user.fullname)</a>
               " "
               <a href=(&profile) class="username">"@" (&article.user.username)</a>
               " · "
               (article.time.map_or_default(formatters::format_relative_time))
            </div>
            for block in blocks(&article.paragraphs) {
               match block {
                  Block::List { ordered: true, items } => {
                     <ol>
                        for item in items {
                           <li>(trusted(inline_html(item, article)))</li>
                        }
                     </ol>
                  },
                  Block::List { ordered: false, items } => {
                     <ul>
                        for item in items {
                           <li>(trusted(inline_html(item, article)))</li>
                        }
                     </ul>
                  },
                  Block::Code(lines) => <pre><code>(lines.join("\n"))</code></pre>,
                  Block::Single(para) => paragraph(para: para, article: article, tweets: tweets, prefs: prefs),
               }
            }
         </article>
      </div>
   })
}

/// Render a single paragraph.
#[component]
async fn paragraph<'a>(
   para: &'a ArticleParagraph,
   article: &'a Article,
   tweets: &'a HashMap<i64, Tweet>,
   prefs: &'a Prefs,
) -> Result<impl View> {
   Ok(view! {
      match para.base_type {
         ArticleBlockType::Atomic => atomic(para: para, article: article, tweets: tweets, prefs: prefs),
         ArticleBlockType::Blockquote => <blockquote>(trusted(inline_html(para, article)))</blockquote>,
         ArticleBlockType::HeaderOne => <h1>(trusted(inline_html(para, article)))</h1>,
         ArticleBlockType::HeaderTwo => <h2>(trusted(inline_html(para, article)))</h2>,
         ArticleBlockType::HeaderThree => <h3>(trusted(inline_html(para, article)))</h3>,
         // CodeBlock is handled by grouping in blocks
         _ => <p>(trusted(inline_html(para, article)))</p>,
      }
   })
}

/// Render an atomic block (media, embedded tweet, markdown code block).
#[component]
async fn atomic<'a>(
   cx: &Cx,
   para: &'a ArticleParagraph,
   article: &'a Article,
   tweets: &'a HashMap<i64, Tweet>,
   prefs: &'a Prefs,
) -> Result<impl View> {
   let config = config(cx);
   let entity = para
      .entity_ranges
      .first()
      .and_then(|range| article.entities.get(range.key));
   let media = entity.map_or_default(|entity| {
      entity
         .media_ids
         .iter()
         .filter_map(|id| article.media.get(id))
         .filter(|media| !media.url.is_empty())
         .collect::<Vec<_>>()
   });

   Ok(view! {
      if let Some(entity) = entity {
         match entity.entity_type {
            ArticleEntityType::Markdown => {
               let (lang, code) = strip_markdown_fences(&entity.markdown);
               <div class="code-block">
                  <div class="code-header">
                     <span class="code-lang">(lang)</span>
                     <button class="copy-btn" type="button" title="Copy">
                        // Two stacked squares (clipboard copy icon).
                        <svg
                           width="16"
                           height="16"
                           viewBox="0 0 24 24"
                           fill="none"
                           stroke="currentColor"
                           stroke-width="2"
                           stroke-linecap="round"
                           stroke-linejoin="round"
                        >
                           <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
                           <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
                        </svg>
                     </button>
                  </div>
                  <pre><code>(code)</code></pre>
               </div>
            },
            ArticleEntityType::Media => {
               for media in media {
                  match media.media_type {
                     ArticleMediaType::ApiImage => {
                        <span class="image">
                           <img
                              src=(formatters::get_pic_url(&media.url, config.config.base64_media))
                              alt=""
                              loading="lazy"
                           >
                        </span>
                     },
                     ArticleMediaType::ApiGif => {
                        <span class="image">
                           <video
                              src=(formatters::get_vid_url(
                                 &media.url,
                                 &config.config.hmac_key,
                                 config.config.base64_media,
                              ))
                              controls=""
                              autoplay=""
                              loop=""
                           ></video>
                        </span>
                     },
                     ArticleMediaType::Unknown => "",
                  }
               }
            },
            ArticleEntityType::Tweet => {
               if let Some(tweet) = entity.tweet_id.parse::<i64>().ok().and_then(|id| tweets.get(&id)) {
                  tweet_item(tweet: tweet, prefs: prefs, main: true)
               }
            },
            ArticleEntityType::Divider => <hr>,
            _ => "",
         }
      }
   })
}

/// Strip markdown code fences (``` or ```lang) from a markdown string,
/// returning the language tag (if any) and the inner code content.
fn strip_markdown_fences(md: &str) -> (&str, &str) {
   let trimmed = md.trim();
   let body = trimmed.strip_prefix("```").unwrap_or(trimmed);
   let (lang, body) = body.find('\n').map_or(("", body), |nl| {
      let tag = body[..nl].trim();
      (tag, &body[nl + 1..])
   });
   let code = body.strip_suffix("```").unwrap_or(body).trim_end();
   (lang, code)
}

/// Render paragraph text with entity ranges (links, twemoji) and inline
/// styles, escaping every piece of text.
fn inline_html(para: &ArticleParagraph, article: &Article) -> String {
   let chars = para.text.chars().collect::<Vec<char>>();
   let mut html = String::new();
   let mut last = 0;

   for range in &para.entity_ranges {
      if range.offset > last {
         styled_html(&mut html, para, &chars, last, range.offset);
      }
      let end = range.offset + range.length;

      if let Some(entity) = article.entities.get(range.key) {
         match entity.entity_type {
            ArticleEntityType::Link => {
               let _ = write!(html, r#"<a href="{}">"#, html_escape(&entity.url));
               styled_html(&mut html, para, &chars, range.offset, end);
               html.push_str("</a>");
            },
            ArticleEntityType::Twemoji => {
               let pic = formatters::get_pic_url(&entity.twemoji, false);
               let _ = write!(
                  html,
                  r#"<img class="twemoji" src="{}" alt="">"#,
                  html_escape(&pic)
               );
            },
            _ => styled_html(&mut html, para, &chars, range.offset, end),
         }
      }
      last = end;
   }

   if last < chars.len() {
      styled_html(&mut html, para, &chars, last, chars.len());
   }
   html
}

/// Append a range of text with inline style ranges applied.
fn styled_html(
   html: &mut String,
   para: &ArticleParagraph,
   chars: &[char],
   start: usize,
   end: usize,
) {
   let mut pos = start;
   while pos < end {
      let mut styles = [false; 3];
      let mut style_end = end;
      for range in &para.inline_style_ranges {
         let range_end = range.offset + range.length;
         if range.offset <= pos && pos < range_end {
            match range.style {
               ArticleStyle::Bold => styles[0] = true,
               ArticleStyle::Italic => styles[1] = true,
               ArticleStyle::Strikethrough => styles[2] = true,
               ArticleStyle::Unknown => {},
            }
         }
         for boundary in [range.offset, range_end] {
            if boundary > pos && boundary < style_end {
               style_end = boundary;
            }
         }
      }

      let chunk = chars
         .get(pos..style_end)
         .map_or_default(|slice| slice.iter().collect::<String>());
      let tags = ["strong", "em", "s"]
         .into_iter()
         .zip(styles)
         .filter_map(|(tag, on)| on.then_some(tag))
         .collect::<Vec<_>>();
      for tag in &tags {
         let _ = write!(html, "<{tag}>");
      }
      html.push_str(&html_escape(&chunk));
      for tag in tags.iter().rev() {
         let _ = write!(html, "</{tag}>");
      }
      pos = style_end;
   }
}
