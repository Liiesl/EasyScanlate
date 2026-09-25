//! Single-profile translation exchange for Settings → Advanced.
//!
//! One `<line>` per visible OCR entry, in the same order the translator sends
//! to the AI (`TranslateItem { filename: file_tag, id, text }`): the immutable
//! OCR `<source>` plus the profile-resolved `<text>` (translation or OCR
//! fallback). Translators edit `<text>` outside the app; import writes it back
//! as profile deltas.
//!
//! `entry_id` is the stable [`easyscanlate_model::EntryId`]; unknown ids on
//! import are returned (not dropped silently) so the caller can report them.

use quick_xml::events::{BytesStart, BytesText, Event};
use quick_xml::Reader;
use quick_xml::Writer;

const VERSION: u32 = 1;

/// One exported line: stable entry id, image file tag, immutable OCR source,
/// and the profile-resolved text (what the AI receives).
#[derive(Debug, Clone)]
pub struct TranslationLine {
    pub id: u64,
    pub file: String,
    pub source: String,
    pub text: String,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn unesc(s: &str) -> String {
    s.replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

fn attr(e: &BytesStart, key: &[u8]) -> Option<String> {
    for a in e.attributes().flatten() {
        if a.key.as_ref() == key
            && let Ok(v) = a.unescape_value()
        {
            return Some(v.into_owned());
        }
    }
    None
}

fn write_text_element(writer: &mut Writer<Vec<u8>>, tag: &str, indent: &str, text: &str) -> Result<(), String> {
    writer
        .write_event(Event::Text(BytesText::from_escaped(indent)))
        .map_err(|e| e.to_string())?;
    writer
        .write_event(Event::Start(BytesStart::new(tag)))
        .map_err(|e| e.to_string())?;
    writer
        .write_event(Event::Text(BytesText::from_escaped(esc(text))))
        .map_err(|e| e.to_string())?;
    writer
        .write_event(Event::End(BytesStart::new(tag).to_end()))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Serialize one profile: every visible line with OCR source + resolved text.
pub fn to_xml_string(profile: &str, lines: &[TranslationLine]) -> Result<String, String> {
    let mut writer = Writer::new(Vec::new());
    writer
        .write_event(Event::Decl(quick_xml::events::BytesDecl::new(
            "1.0",
            Some("UTF-8"),
            None,
        )))
        .map_err(|e| e.to_string())?;
    writer
        .write_event(Event::Text(BytesText::from_escaped("\n")))
        .map_err(|e| e.to_string())?;

    let mut root = BytesStart::new("translation-export");
    root.push_attribute(("version", VERSION.to_string().as_str()));
    root.push_attribute(("profile", esc(profile).as_str()));
    writer
        .write_event(Event::Start(root))
        .map_err(|e| e.to_string())?;

    for line in lines {
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n  ")))
            .map_err(|e| e.to_string())?;
        let mut d = BytesStart::new("line");
        d.push_attribute(("entry_id", line.id.to_string().as_str()));
        d.push_attribute(("file", esc(&line.file).as_str()));
        writer
            .write_event(Event::Start(d))
            .map_err(|e| e.to_string())?;
        write_text_element(&mut writer, "source", "\n    ", &line.source)?;
        write_text_element(&mut writer, "text", "\n    ", &line.text)?;
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n  ")))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::End(BytesStart::new("line").to_end()))
            .map_err(|e| e.to_string())?;
    }
    writer
        .write_event(Event::Text(BytesText::from_escaped("\n")))
        .map_err(|e| e.to_string())?;
    writer
        .write_event(Event::End(BytesStart::new("translation-export").to_end()))
        .map_err(|e| e.to_string())?;
    writer
        .write_event(Event::Text(BytesText::from_escaped("\n")))
        .map_err(|e| e.to_string())?;

    let bytes = writer.into_inner();
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

/// Parse a `<translation-export>` document.
///
/// Returns `(profile_name, lines)`. Accepts the current `<line entry_id
/// file><source>..<text>` rows and the previous `<delta
/// entry_id><translation>` rows (`<translation>` maps to `text` with an empty
/// source; `<text>` wins if both are present). Duplicate ids keep document
/// order so the caller can last-win. Errors when the root element or profile
/// is absent.
pub fn from_xml_str(s: &str) -> Result<(String, Vec<TranslationLine>), String> {
    let mut reader = Reader::from_str(s);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut profile: Option<String> = None;
    let mut lines: Vec<TranslationLine> = Vec::new();
    let mut cur_id: Option<u64> = None;
    let mut cur_file = String::new();
    let mut cur_source: Option<String> = None;
    let mut cur_text: Option<String> = None;
    let mut collecting: Option<&str> = None;
    let mut text_buf = String::new();
    let mut depth = 0usize;

    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => return Err(format!("xml read error: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match name.as_str() {
                    "translation-export" => {
                        if depth == 0 {
                            let raw = attr(&e, b"profile").unwrap_or_default();
                            let raw = unesc(&raw);
                            if raw.trim().is_empty() {
                                return Err("translation file is missing profile=\"Name\"".to_string());
                            }
                            profile = Some(raw);
                        }
                        depth += 1;
                    }
                    "line" | "delta" => {
                        depth += 1;
                        let id = attr(&e, b"entry_id")
                            .and_then(|v| v.parse::<u64>().ok())
                            .unwrap_or(u64::MAX);
                        cur_id = Some(id);
                        cur_file = attr(&e, b"file").map(|v| unesc(&v)).unwrap_or_default();
                        cur_source = None;
                        cur_text = None;
                    }
                    "source" | "text" | "translation" => {
                        depth += 1;
                        collecting = Some(match name.as_str() {
                            "source" => "source",
                            _ => "text",
                        });
                        text_buf.clear();
                    }
                    _ => {
                        depth += 1;
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "translation-export" && depth == 0 {
                    let raw = attr(&e, b"profile").unwrap_or_default();
                    let raw = unesc(&raw);
                    if raw.trim().is_empty() {
                        return Err("translation file is missing profile=\"Name\"".to_string());
                    }
                    profile = Some(raw);
                } else if name == "source" && cur_id.is_some() {
                    cur_source = Some(String::new());
                } else if (name == "text" || name == "translation") && cur_id.is_some() {
                    // `<text>` wins over `<translation>` when both are present.
                    if name == "text" || cur_text.is_none() {
                        cur_text = Some(String::new());
                    }
                }
            }
            Ok(Event::Text(e)) => {
                if collecting.is_some()
                    && let Ok(txt) = e.unescape()
                {
                    text_buf.push_str(&txt);
                }
            }
            Ok(Event::CData(e)) => {
                if collecting.is_some()
                    && let Ok(txt) = std::str::from_utf8(&e.into_inner())
                {
                    text_buf.push_str(txt);
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match name.as_str() {
                    "source" => {
                        if collecting == Some("source") {
                            cur_source = Some(unesc(&text_buf));
                            collecting = None;
                            text_buf.clear();
                        }
                        depth = depth.saturating_sub(1);
                    }
                    "text" => {
                        if collecting == Some("text") {
                            cur_text = Some(unesc(&text_buf));
                            collecting = None;
                            text_buf.clear();
                        }
                        depth = depth.saturating_sub(1);
                    }
                    "translation" => {
                        // Legacy rows: only fill when `<text>` hasn't already.
                        if collecting == Some("text") {
                            if cur_text.is_none() {
                                cur_text = Some(unesc(&text_buf));
                            }
                            collecting = None;
                            text_buf.clear();
                        }
                        depth = depth.saturating_sub(1);
                    }
                    "line" | "delta" => {
                        if let Some(id) = cur_id.take() {
                            if id != u64::MAX {
                                lines.push(TranslationLine {
                                    id,
                                    file: std::mem::take(&mut cur_file),
                                    source: cur_source.take().unwrap_or_default(),
                                    text: cur_text.take().unwrap_or_default(),
                                });
                            } else {
                                cur_source.take();
                                cur_text.take();
                                cur_file.clear();
                            }
                        }
                        depth = depth.saturating_sub(1);
                    }
                    _ => {
                        depth = depth.saturating_sub(1);
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }

    let Some(profile) = profile else {
        return Err("not a translation file: missing <translation-export>".to_string());
    };
    Ok((profile, lines))
}
