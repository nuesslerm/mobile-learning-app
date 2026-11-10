//! Marking a handwritten Tamil letter (`routes/tamilCheck.ts`): the picture the vision model is shown, the question it is asked, and
//! the tidying of its answer. The picture is one PNG, two panels wide: the letter as printed in the app's own face on the left, the
//! learner's ink on the right, each trimmed to what is drawn and fitted inside the same margin so print and writing are the same
//! size whatever the word's length or where on the pad it was written.

use crate::js::{is_js_space, js_string, truthy};
use image::codecs::png::PngEncoder;
use image::{imageops, ImageEncoder, Rgba, RgbaImage};
use resvg::usvg::fontdb;
use resvg::{tiny_skia, usvg};
use serde_json::{json, Value};
use std::sync::{Arc, OnceLock};

/// Each panel of the picture the model sees: the printed letter, the writing.
const PANEL: u32 = 448;
const GUTTER: u32 = 24;
/// Clear space around each drawing, as a share of its panel.
const PANEL_MARGIN: f64 = 0.08;
const REFERENCE_FONT_SIZE: u32 = 200;
/// The ink is drawn at twice its size, as sharp does at 144 dpi; a pad so large that this passes the cap is drawn smaller.
const SCALE: f32 = 2.0;
const MAX_PIXELS: f64 = 16_000_000.0;
const FONT: &[u8] = include_bytes!("../../../../frontend/mla-tamagui/assets/fonts/NotoSansTamil-Regular.ttf");

fn fonts() -> Arc<fontdb::Database> {
    static DB: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    Arc::clone(DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_font_data(FONT.to_vec());
        Arc::new(db)
    }))
}

fn render(svg: &[u8]) -> Result<RgbaImage, String> {
    let options = usvg::Options { fontdb: fonts(), ..usvg::Options::default() };
    let tree = usvg::Tree::from_data(svg, &options).map_err(|e| e.to_string())?;
    let size = tree.size().to_int_size();
    let area = f64::from(size.width()) * f64::from(size.height());
    let scale = SCALE.min((MAX_PIXELS / area).sqrt() as f32);
    let (width, height) = (((size.width() as f32 * scale) as u32).max(1), ((size.height() as f32 * scale) as u32).max(1));
    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or("the drawing has no size")?;
    pixmap.fill(tiny_skia::Color::WHITE);
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    RgbaImage::from_raw(width, height, pixmap.take()).ok_or_else(|| "the drawing could not be read back".to_string())
}

/// The picture cut to the box around everything that is not white (a channel more than `threshold` from it); an empty drawing is kept whole.
fn trim(image: &RgbaImage, threshold: i32) -> RgbaImage {
    let (width, height) = image.dimensions();
    let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);
    for (x, y, pixel) in image.enumerate_pixels() {
        let distance = (255 - i32::from(pixel[0])).max(255 - i32::from(pixel[1])).max(255 - i32::from(pixel[2]));
        if distance > threshold {
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }
    if right < left {
        return image.clone();
    }
    imageops::crop_imm(image, left, top, right - left + 1, bottom - top + 1).to_image()
}

fn panel(svg: &[u8]) -> Result<RgbaImage, String> {
    let inner = (f64::from(PANEL) * (1.0 - 2.0 * PANEL_MARGIN)).round() as u32;
    let edge = (PANEL - inner) / 2;
    let trimmed = trim(&render(svg)?, 16);
    let (width, height) = trimmed.dimensions();
    let fit = (f64::from(inner) / f64::from(width)).min(f64::from(inner) / f64::from(height));
    let (new_width, new_height) = (((f64::from(width) * fit).round() as u32).max(1), ((f64::from(height) * fit).round() as u32).max(1));
    let fitted = imageops::resize(&trimmed, new_width, new_height, imageops::FilterType::Lanczos3);
    let mut out = RgbaImage::from_pixel(PANEL, PANEL, Rgba([255, 255, 255, 255]));
    imageops::overlay(&mut out, &fitted, i64::from(edge + (inner - new_width) / 2), i64::from(edge + (inner - new_height) / 2));
    Ok(out)
}

/// On a canvas wide enough for any word, then trimmed and fitted: a size guessed from the text's length clipped இலை at both edges, so
/// the model compared the writing with half a word.
fn reference_svg(glyph: &str) -> String {
    let width = PANEL * 4;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{PANEL}\"><rect width=\"{width}\" height=\"{PANEL}\" fill=\"#fff\"/><text x=\"{}\" y=\"{}\" font-family=\"Noto Sans Tamil\" font-size=\"{REFERENCE_FONT_SIZE}\" text-anchor=\"middle\" fill=\"#000\">{glyph}</text></svg>",
        width / 2,
        (f64::from(PANEL) * 0.68).round() as u32
    )
}

/// One PNG, two panels: the printed letter on the left, the writing on the right.
pub fn rasterise(ink_svg: &str, glyph: &str) -> Result<Vec<u8>, String> {
    let reference = panel(reference_svg(glyph).as_bytes())?;
    let ink = panel(ink_svg.as_bytes())?;
    let mut picture = RgbaImage::from_pixel(PANEL * 2 + GUTTER, PANEL, Rgba([255, 255, 255, 255]));
    imageops::overlay(&mut picture, &reference, 0, 0);
    // A grey rule between the panels, so "left" and "right" are unambiguous.
    for y in 0..PANEL {
        for x in (PANEL + GUTTER / 2 - 1)..(PANEL + GUTTER / 2 + 1) {
            picture.put_pixel(x, y, Rgba([153, 153, 153, 255]));
        }
    }
    imageops::overlay(&mut picture, &ink, i64::from(PANEL + GUTTER), 0);
    let rgb = image::DynamicImage::ImageRgba8(picture).to_rgb8();
    let mut png = Vec::new();
    PngEncoder::new(&mut png).write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
    Ok(png)
}

/// The model is asked to COMPARE, not to recognise. Live against MiniMax-M3 with the writing alone, every correct அ came back wrong:
/// it described அ as "a small hook" and marked the learner down for the loop and the stem the letter actually has. With the printed
/// letter beside the writing the question becomes "is the right-hand shape the left-hand shape", which a vision model can answer
/// without knowing Tamil at all.
pub fn prompt(glyph: &str, transliteration: &str, is_word: bool) -> String {
    // A word named a letter had the model judge இலை as one shape, its three letters as parts.
    let what = if is_word { "word" } else { "letter" };
    [
        format!("The image has two panels. LEFT: the Tamil {what} as printed in a font."),
        format!("RIGHT: the same {what} as a learner wrote it with a finger on a blank pad."),
        format!("The {what} is \"{glyph}\" ({transliteration})."),
        String::new(),
        format!("Judge only whether the RIGHT panel reads as the LEFT panel's {what} to"),
        "someone who reads Tamil. Compare shapes: the loops, the bars, where the".into(),
        "stems are, how many parts there are. Ignore size, position, line thickness".into(),
        "and neatness -- a finger is shaky, and shaky is fine. Missing a part,".into(),
        "adding a part, or a shape that reads as a DIFFERENT Tamil letter is wrong".into(),
        "however tidy it is.".into(),
        String::new(),
        "Be lenient: a small slip -- a dot or mark a little off, a stroke short or".into(),
        "wobbly -- still counts when a reader would recognise the letter. Say".into(),
        "ok=false only when it would be read as something else or a whole part is".into(),
        "missing. When you say ok=false, the summary must name the exact part that".into(),
        "is wrong, missing or extra, so a reviewer can check the objection.".into(),
        String::new(),
        "Answer with JSON and nothing else:".into(),
        "{\"ok\": true|false, \"score\": 0-100, \"summary\": \"one sentence comparing right to left\",".into(),
        " \"fix\": \"one sentence on the single thing to change, or an empty string\"}".into(),
    ]
    .join("\n")
}

/// `Number(value)` for the score the model reported; `NaN` when it is not a number at all.
fn js_number(value: &Value) -> f64 {
    match value {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Null => 0.0,
        Value::String(s) => {
            let text = s.trim_matches(is_js_space);
            if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                return u64::from_str_radix(hex, 16).map_or(f64::NAN, |n| n as f64);
            }
            match text {
                "" => 0.0,
                "Infinity" | "+Infinity" => f64::INFINITY,
                "-Infinity" => f64::NEG_INFINITY,
                // Rust reads `inf` and `nan` as numbers, JavaScript does not.
                _ if text.chars().any(|c| c.is_ascii_alphabetic() && c != 'e' && c != 'E') => f64::NAN,
                _ => text.parse::<f64>().unwrap_or(f64::NAN),
            }
        }
        Value::Array(_) | Value::Object(_) => f64::NAN,
    }
}

/// `ok`, `score`, `summary` and `fix` read one by one from JSON that does not parse whole; `None` without ok or score.
fn lenient_fields(text: &str) -> Option<Value> {
    let after = |key: &str| text.find(&format!("\"{key}\"")).map(|i| text[i + key.len() + 2..].trim_start().trim_start_matches(':').trim_start());
    let string_at = |rest: &str| -> Option<String> {
        let mut out = String::new();
        let mut chars = rest.strip_prefix('"')?.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => return Some(out),
                '\\' => out.push(match chars.next()? { 'n' => '\n', 't' => '\t', other => other }),
                c => out.push(c),
            }
        }
        Some(format!("{}…", out.trim_end()))
    };
    let mut fields = serde_json::Map::new();
    if let Some(rest) = after("ok") {
        if rest.starts_with("true") || rest.starts_with("false") {
            fields.insert("ok".into(), json!(rest.starts_with("true")));
        }
    }
    if let Some(rest) = after("score") {
        let number: String = rest.trim_start_matches('"').chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
        if !number.is_empty() {
            fields.insert("score".into(), json!(number));
        }
    }
    if !fields.contains_key("ok") && !fields.contains_key("score") {
        return None;
    }
    for key in ["summary", "fix"] {
        if let Some(text) = after(key).and_then(string_at) {
            fields.insert(key.into(), json!(text));
        }
    }
    Some(Value::Object(fields))
}

/// The model's reply with the JSON dug out of whatever it wrapped it in.
pub fn parse_verdict(reply: &str) -> Result<Value, String> {
    let (Some(start), Some(end)) = (reply.find('{'), reply.rfind('}')) else {
        return Err(format!("no JSON in the model's reply: {}", reply.chars().take(200).collect::<String>()));
    };
    if end <= start {
        return Err(format!("no JSON in the model's reply: {}", reply.chars().take(200).collect::<String>()));
    }
    let parsed: Value = match serde_json::from_str(&reply[start..=end]) {
        Ok(v) => v,
        // a reply cut off mid-sentence still carries its ok and score
        Err(e) => lenient_fields(&reply[start..]).ok_or_else(|| format!("{e} in the model's reply: {}", reply.chars().take(300).collect::<String>()))?,
    };
    let field = |name: &str| parsed.get(name).filter(|v| !v.is_null());
    let raw = field("score").map_or(0.0, js_number);
    let score = (raw + 0.5).floor().clamp(0.0, 100.0);
    let score = if raw.is_nan() { f64::NAN } else { score };
    let summary = match field("summary").and_then(Value::as_str).map(str::trim) {
        Some(text) if !text.is_empty() => text.to_string(),
        _ => "No comment came back.".into(),
    };
    let fix = field("fix").and_then(Value::as_str).map(str::trim).unwrap_or_default().to_string();
    // 80 or more passes whatever the flag says: "not yet, 82" contradicts itself.
    let ok = (parsed.get("ok").is_some_and(truthy) && score >= 60.0) || score >= 80.0;
    let mut verdict = json!({ "ok": ok, "score": if score.is_nan() { Value::Null } else { json!(score as i64) }, "summary": summary });
    if !fix.is_empty() {
        verdict["fix"] = json!(fix);
    }
    Ok(verdict)
}

/// What a request names a character: a word when the id starts `word-`.
pub fn is_word(character_id: Option<&Value>) -> bool {
    character_id.is_some_and(|id| js_string(id).starts_with("word-"))
}
