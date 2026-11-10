//! The file preview's markdown converter: `pulldown-cmark` (CommonMark with tables and strikethrough) with two guards the page needs.
//! Raw HTML in a document is shown as text, so a file cannot put markup into the preview, and a link opens in a new tab and never
//! carries a script URL.

use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag, TagEnd};

/// A link target the page may follow: anything but a script-bearing scheme.
fn is_safe_target(url: &str) -> bool {
    let scheme: String = url.trim_start().chars().take_while(|c| *c != ':' && *c != '/' && *c != '?' && *c != '#').collect::<String>().to_ascii_lowercase();
    !matches!(scheme.as_str(), "javascript" | "data" | "vbscript")
}

pub fn md_to_html(md: &str) -> String {
    let events = Parser::new_ext(md, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH).map(|event| match event {
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        Event::Start(Tag::Link { dest_url, .. }) => {
            let target = if is_safe_target(&dest_url) { dest_url } else { CowStr::from("#") };
            Event::Html(format!("<a href=\"{}\" target=\"_blank\" rel=\"noopener\">", html_escape::encode_double_quoted_attribute(&target)).into())
        }
        Event::End(TagEnd::Link) => Event::Html("</a>".into()),
        other => other,
    });
    let mut out = String::with_capacity(md.len() * 3 / 2);
    html::push_html(&mut out, events);
    out
}
