//! The text of a Word document, read as the zip of XML it is.

use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::{Cursor, Read};
use zip::ZipArchive;

/// The raw text of a Word document, as mammoth's `extractRawText`: every paragraph followed by a blank line, tabs kept, line
/// breaks and everything that is not text dropped.
pub fn docx_text(bytes: &[u8]) -> Result<String, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|_| "Could not find main document part. Are you sure this is a valid .docx file?".to_string())?
        .read_to_string(&mut xml)
        .map_err(|e| e.to_string())?;
    let mut reader = Reader::from_str(&xml);
    let (mut out, mut in_text) = (String::new(), false);
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) if e.name().as_ref() == b"w:t" => in_text = true,
            Event::End(e) if e.name().as_ref() == b"w:t" => in_text = false,
            Event::End(e) if e.name().as_ref() == b"w:p" => out.push_str("\n\n"),
            Event::Empty(e) if e.name().as_ref() == b"w:tab" => out.push('\t'),
            Event::Text(t) if in_text => out.push_str(&t.unescape().map_err(|e| e.to_string())?),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}
