use std::{
   collections::HashMap,
   mem,
};

use maud::{
   Markup,
   PreEscaped,
   html,
};

use super::renderutils;
use crate::{
   config::Config,
   types::{
      Article,
      ArticleBlockType,
      ArticleEntityType,
      ArticleMediaType,
      ArticleParagraph,
      ArticleStyle,
      Prefs,
      Tweet,
   },
   utils::formatters,
};

/// Two stacked squares (clipboard copy icon).
const COPY_ICON_SVG: &str = r#"<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>"#;

/// Render a Twitter Article (Notes) page.
pub fn render_note(
   article: &Article,
   tweets: &HashMap<i64, Tweet>,
   config: &Config,
   prefs: Option<&Prefs>,
) -> Markup {
   let cover = if article.cover_image.is_empty() {
      String::new()
   } else {
      formatters::get_pic_url(&article.cover_image, config.config.base64_media)
   };

   let time_str = article
      .time
      .map(formatters::format_relative_time)
      .unwrap_or_default();

   html! {
       div class="note" {
           @if !cover.is_empty() {
               img class="cover" src=(cover) alt="" loading="lazy";
           }

           article {
               h1 { (article.title) }

               div class="author" {
                   (renderutils::gen_img(&formatters::get_user_pic(&article.user.user_pic, "_mini"), "avatar round mini", config))
                   " "
                   a href=(format!("/{}", article.user.username)) class="fullname" {
                       (article.user.fullname)
                   }
                   " "
                   a href=(format!("/{}", article.user.username)) class="username" {
                       "@" (article.user.username)
                   }
                   " · "
                   (time_str)
               }

               (render_paragraphs(article, tweets, config, prefs))
           }
       }
   }
}

/// Render all paragraphs, grouping list items into `<ul>`/`<ol>` and
/// consecutive code-block lines into `<pre><code>`.
fn render_paragraphs(
   article: &Article,
   tweets: &HashMap<i64, Tweet>,
   config: &Config,
   prefs: Option<&Prefs>,
) -> Markup {
   let mut parts: Vec<Markup> = Vec::new();
   let mut cur_list_type: Option<ArticleBlockType> = None;
   let mut list_items: Vec<Markup> = Vec::new();
   let mut code_lines: Vec<String> = Vec::new();

   let flush_list =
      |lt: &mut Option<ArticleBlockType>, items: &mut Vec<Markup>, out: &mut Vec<Markup>| {
         if items.is_empty() {
            return;
         }
         let taken = mem::take(items);
         let kind = lt.take();
         out.push(html! {
             @if kind == Some(ArticleBlockType::OrderedListItem) {
                 ol {
                     @for item in &taken {
                         (item)
                     }
                 }
             } @else {
                 ul {
                     @for item in &taken {
                         (item)
                     }
                 }
             }
         });
      };

   let flush_code = |lines: &mut Vec<String>, out: &mut Vec<Markup>| {
      if lines.is_empty() {
         return;
      }
      let code = mem::take(lines).join("\n");
      out.push(html! { pre { code { (code) } } });
   };

   for para in &article.paragraphs {
      let is_list = matches!(
         para.base_type,
         ArticleBlockType::OrderedListItem | ArticleBlockType::UnorderedListItem
      );

      if para.base_type == ArticleBlockType::CodeBlock {
         flush_list(&mut cur_list_type, &mut list_items, &mut parts);
         code_lines.push(para.text.clone());
      } else if is_list {
         flush_code(&mut code_lines, &mut parts);
         if cur_list_type.is_some() && cur_list_type != Some(para.base_type) {
            flush_list(&mut cur_list_type, &mut list_items, &mut parts);
         }
         cur_list_type = Some(para.base_type);
         list_items.push(render_paragraph(para, article, tweets, config, prefs));
      } else {
         flush_code(&mut code_lines, &mut parts);
         flush_list(&mut cur_list_type, &mut list_items, &mut parts);
         parts.push(render_paragraph(para, article, tweets, config, prefs));
      }
   }

   flush_code(&mut code_lines, &mut parts);
   flush_list(&mut cur_list_type, &mut list_items, &mut parts);

   html! {
       @for part in &parts {
           (part)
       }
   }
}

/// Render a single paragraph.
fn render_paragraph(
   para: &ArticleParagraph,
   article: &Article,
   tweets: &HashMap<i64, Tweet>,
   config: &Config,
   _prefs: Option<&Prefs>,
) -> Markup {
   if para.base_type == ArticleBlockType::Atomic {
      return render_atomic(para, article, tweets, config);
   }

   let inner = render_text_with_entities(para, article);

   match para.base_type {
      ArticleBlockType::Blockquote => html! { blockquote { (inner) } },
      ArticleBlockType::HeaderOne => html! { h1 { (inner) } },
      ArticleBlockType::HeaderTwo => html! { h2 { (inner) } },
      ArticleBlockType::HeaderThree => html! { h3 { (inner) } },
      ArticleBlockType::OrderedListItem | ArticleBlockType::UnorderedListItem => {
         html! { li { (inner) } }
      },
      // CodeBlock is handled by grouping in render_paragraphs
      _ => html! { p { (inner) } },
   }
}

/// Render an atomic block (media, embedded tweet, markdown code block).
fn render_atomic(
   para: &ArticleParagraph,
   article: &Article,
   tweets: &HashMap<i64, Tweet>,
   config: &Config,
) -> Markup {
   let Some(er) = para.entity_ranges.first() else {
      return html! {};
   };
   let Some(entity) = article.entities.get(er.key) else {
      return html! {};
   };

   match entity.entity_type {
      ArticleEntityType::Markdown => {
         let (lang, code) = strip_markdown_fences(&entity.markdown);
         html! {
             div class="code-block" {
                 div class="code-header" {
                     span class="code-lang" { (lang) }
                     button class="copy-btn" onclick="navigator.clipboard.writeText(this.closest('.code-block').querySelector('code').textContent).then(()=>{let t=document.createElement('div');t.className='copy-toast';t.textContent='Copied!';this.closest('.code-block').appendChild(t);setTimeout(()=>t.remove(),1500)})" title="Copy" {
                         (PreEscaped(COPY_ICON_SVG))
                     }
                 }
                 pre { code { (code) } }
             }
         }
      },
      ArticleEntityType::Media => {
         let mut markup_parts = Vec::new();
         for id in &entity.media_ids {
            let Some(media) = article.media.get(id) else {
               continue;
            };
            if media.url.is_empty() {
               continue;
            }
            match media.media_type {
               ArticleMediaType::ApiImage => {
                  let pic = formatters::get_pic_url(&media.url, config.config.base64_media);
                  markup_parts.push(html! {
                      span class="image" {
                          img src=(pic) alt="" loading="lazy";
                      }
                  });
               },
               ArticleMediaType::ApiGif => {
                  let vid = formatters::get_vid_url(
                     &media.url,
                     &config.config.hmac_key,
                     config.config.base64_media,
                  );
                  markup_parts.push(html! {
                      span class="image" {
                          video src=(vid) controls autoplay loop {};
                      }
                  });
               },
               ArticleMediaType::Unknown => {},
            }
         }
         html! { @for markup in &markup_parts { (markup) } }
      },
      ArticleEntityType::Tweet => {
         let tweet_id = entity.tweet_id.parse::<i64>().unwrap_or(0);
         tweets.get(&tweet_id).map_or_else(
            || html! {},
            |tweet| super::tweet::TweetRenderer::new(tweet, config, true).render(),
         )
      },
      ArticleEntityType::Divider => html! { hr; },
      _ => html! {},
   }
}

/// Strip markdown code fences (``` or ```lang) from a markdown string,
/// returning the language tag (if any) and the inner code content.
fn strip_markdown_fences(md: &str) -> (&str, &str) {
   let trimmed = md.trim();
   let body = trimmed.strip_prefix("```").unwrap_or(trimmed);
   // Extract language tag from first line (e.g. "xml\n...")
   let (lang, body) = body.find('\n').map_or(("", body), |nl| {
      let tag = body[..nl].trim();
      (tag, &body[nl + 1..])
   });
   let code = body.strip_suffix("```").unwrap_or(body).trim_end();
   (lang, code)
}

/// Render paragraph text with entity ranges (links, twemoji) and inline
/// styles.
fn render_text_with_entities(para: &ArticleParagraph, article: &Article) -> Markup {
   let text = &para.text;
   let chars: Vec<char> = text.chars().collect();
   let mut parts: Vec<Markup> = Vec::new();
   let mut last = 0;

   for er in &para.entity_ranges {
      if er.offset > last {
         parts.push(render_styled_text(para, &chars, last, er.offset));
      }

      let Some(entity) = article.entities.get(er.key) else {
         last = er.offset + er.length;
         continue;
      };

      let styled_inner = render_styled_text(para, &chars, er.offset, er.offset + er.length);

      match entity.entity_type {
         ArticleEntityType::Link => {
            parts.push(html! { a href=(entity.url) { (styled_inner) } });
         },
         ArticleEntityType::Twemoji => {
            let pic = formatters::get_pic_url(&entity.twemoji, false);
            parts.push(html! { img class="twemoji" src=(pic) alt=""; });
         },
         _ => {
            parts.push(styled_inner);
         },
      }

      last = er.offset + er.length;
   }

   if last < chars.len() {
      parts.push(render_styled_text(para, &chars, last, chars.len()));
   }

   html! { @for part in &parts { (part) } }
}

/// Render a range of text with inline style ranges applied.
fn render_styled_text(para: &ArticleParagraph, chars: &[char], start: usize, end: usize) -> Markup {
   if para.inline_style_ranges.is_empty() {
      let text: String = chars
         .get(start..end)
         .map(|slice| slice.iter().collect())
         .unwrap_or_default();
      return html! { (text) };
   }

   let mut parts = Vec::new();
   let mut pos = start;

   while pos < end {
      // Determine style at this position
      let mut is_bold = false;
      let mut is_italic = false;
      let mut is_strike = false;

      for sr in &para.inline_style_ranges {
         if sr.offset <= pos && pos < sr.offset + sr.length {
            match sr.style {
               ArticleStyle::Bold => is_bold = true,
               ArticleStyle::Italic => is_italic = true,
               ArticleStyle::Strikethrough => is_strike = true,
               ArticleStyle::Unknown => {},
            }
         }
      }

      // Find how far this style extends
      let mut style_end = end;
      for sr in &para.inline_style_ranges {
         let sr_start = sr.offset;
         let sr_end = sr.offset + sr.length;
         if sr_start > pos && sr_start < style_end {
            style_end = sr_start;
         }
         if sr_end > pos && sr_end < style_end {
            style_end = sr_end;
         }
      }

      let chunk: String = chars
         .get(pos..style_end)
         .map(|slice| slice.iter().collect())
         .unwrap_or_default();

      let mut markup = html! { (chunk) };
      if is_strike {
         markup = html! { s { (markup) } };
      }
      if is_italic {
         markup = html! { em { (markup) } };
      }
      if is_bold {
         markup = html! { strong { (markup) } };
      }
      parts.push(markup);

      pos = style_end;
   }

   html! { @for part in &parts { (part) } }
}
