//! Behavior of the pure compute the service renders with. Given/When/Then, through the public API only.

use crate::{docx::docx_text, highlight, js, md::md_to_html, tamil};
use serde_json::json;
use std::io::{Cursor, Write};

fn docx_with(document_xml: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut archive = zip::ZipWriter::new(Cursor::new(&mut bytes));
        archive.start_file::<_, ()>("word/document.xml", zip::write::SimpleFileOptions::default()).unwrap();
        archive.write_all(document_xml.as_bytes()).unwrap();
        archive.finish().unwrap();
    }
    bytes
}

#[test]
fn given_raw_html_in_markdown_when_converted_then_it_is_shown_as_text() {
    let html = md_to_html("<script>alert(1)</script>");
    assert!(!html.contains("<script>"), "{html}");
    assert!(html.contains("&lt;script&gt;"), "{html}");
}

#[test]
fn given_a_script_link_when_converted_then_it_points_nowhere_and_opens_in_a_new_tab() {
    let html = md_to_html("[a](javascript:alert(1)) [b](https://example.com/x?a=1&b=2)");
    assert!(html.contains("<a href=\"#\" target=\"_blank\" rel=\"noopener\">a</a>"), "{html}");
    assert!(html.contains("href=\"https://example.com/x?a=1&amp;b=2\" target=\"_blank\""), "{html}");
}

#[test]
fn given_a_table_and_strikethrough_when_converted_then_both_render() {
    let html = md_to_html("| h |\n|---|\n| c |\n\n~~gone~~");
    assert!(html.contains("<table>") && html.contains("<del>gone</del>"), "{html}");
}

#[test]
fn given_a_word_document_when_read_then_paragraphs_end_with_a_blank_line_and_tabs_stay() {
    let xml = r#"<w:document><w:body><w:p><w:r><w:t>one</w:t><w:tab/><w:t>two &amp; three</w:t></w:r></w:p><w:p><w:r><w:t>next</w:t></w:r></w:p></w:body></w:document>"#;
    assert_eq!(docx_text(&docx_with(xml)).unwrap(), "one\ttwo & three\n\nnext\n\n");
}

#[test]
fn given_a_zip_without_the_main_part_when_read_then_the_error_names_the_docx() {
    let mut bytes = Vec::new();
    {
        let mut archive = zip::ZipWriter::new(Cursor::new(&mut bytes));
        archive.start_file::<_, ()>("other.xml", zip::write::SimpleFileOptions::default()).unwrap();
        archive.finish().unwrap();
    }
    assert!(docx_text(&bytes).unwrap_err().contains("valid .docx"));
}

#[test]
fn given_bytes_that_are_no_zip_when_read_then_it_is_an_error() {
    assert!(docx_text(b"plain text").is_err());
}

#[test]
fn given_file_names_when_the_language_is_asked_then_the_extension_decides_case_blind() {
    assert_eq!(highlight::language_for_file("src/App.TSX"), "typescript");
    assert_eq!(highlight::language_for_file("main.rs"), "rust");
    assert_eq!(highlight::language_for_file("notes.unknown"), "text");
}

#[test]
fn given_rust_lines_when_highlighted_then_one_html_line_each_with_the_keyword_marked_and_text_escaped() {
    let lines = vec!["fn main() {".to_string(), "    let a = 1 < 2;".to_string()];
    let out = highlight::highlight_lines(&lines, "rust");
    assert_eq!(out.len(), 2);
    assert!(out[0].contains("<span") && out[0].contains("fn"), "{}", out[0]);
    assert!(out[1].contains("&lt;") && !out[1].contains(" < "), "{}", out[1]);
}

#[test]
fn given_an_unknown_language_when_highlighted_then_the_lines_come_back_escaped_and_plain() {
    let out = highlight::highlight_lines(&["<b>&".to_string()], "text");
    assert_eq!(out, vec!["&lt;b&gt;&amp;".to_string()]);
}

#[test]
fn given_a_word_character_id_when_asked_then_it_is_a_word() {
    assert!(tamil::is_word(Some(&json!("word-அம்மா"))));
    assert!(!tamil::is_word(Some(&json!("letter-அ"))));
    assert!(!tamil::is_word(None));
}

#[test]
fn given_a_model_reply_when_parsed_then_a_low_score_is_not_ok_even_if_it_says_so_and_a_high_one_is_ok_anyway() {
    let low = tamil::parse_verdict(r#"{"ok":true,"score":40,"summary":"x"}"#).unwrap();
    assert_eq!(low["ok"], json!(false));
    let high = tamil::parse_verdict(r#"{"ok":false,"score":85,"summary":"x"}"#).unwrap();
    assert_eq!(high["ok"], json!(true));
}

#[test]
fn given_ink_and_a_glyph_when_rasterised_then_the_picture_is_a_png() {
    let ink = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200"><path d="M20 20 L180 180" stroke="black" stroke-width="8" fill="none"/></svg>"#;
    let png = tamil::rasterise(ink, "அ").unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn given_json_values_when_converted_the_way_javascript_does_then_the_strings_match() {
    assert_eq!(js::js_string(&json!(3.0)), "3");
    assert_eq!(js::js_string(&json!([1, null, "a"])), "1,,a");
    assert_eq!(js::js_string(&json!({})), "[object Object]");
    assert!(!js::truthy(&json!(0)) && !js::truthy(&json!("")) && js::truthy(&json!([])));
    assert!(js::is_js_space('\u{a0}') && !js::is_js_space('x'));
}
