//! XML serialization for easyscanlate-model Project.
//! Rust-native names, snake_case. Single `project.xml`.
//!
//! `ImageMeta.path` is stored zip-relative (`images/<id>_<basename>`) inside
//! `.mmtl`; at runtime `mmtl/src/zip.rs` resolves it to an absolute temp path.

use std::collections::HashMap;

use quick_xml::events::{BytesStart, BytesText, Event};
use quick_xml::Reader;
use quick_xml::Writer;
use easyscanlate_model::{
    EntryId, EntrySource, EntryStyle, Extras, ImageId, ImageMeta, InpaintId, InpaintPatch,
    OcrEntry, OcrResult, Profile, ProfileId, Project, Quad, Shape, ShapeKind, TextAlign,
    TextGradientDir,
};

const VERSION: u32 = 1;

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
        if a.key.as_ref() == key {
            if let Ok(v) = a.unescape_value() {
                return Some(v.into_owned());
            }
        }
    }
    None
}

fn parse_f32(s: &str) -> f32 {
    s.parse::<f32>().unwrap_or(0.0)
}
fn parse_u64(s: &str) -> u64 {
    s.parse::<u64>().unwrap_or(0)
}
#[allow(dead_code)]
fn parse_u32(s: &str) -> u32 {
    s.parse::<u32>().unwrap_or(0)
}
fn parse_u8(s: &str) -> u8 {
    s.parse::<u8>().unwrap_or(0)
}
#[allow(dead_code)]
fn parse_f32_opt(s: Option<String>) -> f32 {
    s.as_deref().map(parse_f32).unwrap_or(0.0)
}
#[allow(dead_code)]
fn parse_u64_opt(s: Option<String>) -> u64 {
    s.as_deref().map(parse_u64).unwrap_or(0)
}

#[allow(dead_code)]
fn rgba_to_str(c: [u8; 4]) -> String {
    format!("{},{},{},{}", c[0], c[1], c[2], c[3])
}
#[allow(dead_code)]
fn str_to_rgba(s: &str) -> [u8; 4] {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() == 4 {
        [
            parse_u8(parts[0].trim()),
            parse_u8(parts[1].trim()),
            parse_u8(parts[2].trim()),
            parse_u8(parts[3].trim()),
        ]
    } else {
        [0, 0, 0, 255]
    }
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

pub fn to_xml_string(project: &Project) -> Result<String, String> {
    let mut writer = Writer::new(Vec::new());
    // xml decl
    writer
        .write_event(Event::Decl(quick_xml::events::BytesDecl::new(
            "1.0", Some("UTF-8"), None,
        )))
        .map_err(|e| e.to_string())?;
    writer
        .write_event(Event::Text(BytesText::from_escaped("\n")))
        .map_err(|e| e.to_string())?;

    let mut root = BytesStart::new("project");
    root.push_attribute(("version", VERSION.to_string().as_str()));
    writer
        .write_event(Event::Start(root))
        .map_err(|e| e.to_string())?;
    writer
        .write_event(Event::Text(BytesText::from_escaped("\n  ")))
        .map_err(|e| e.to_string())?;

    // images
    {
        let mut el = BytesStart::new("images");
        el.push_attribute(("next_image_id", project.next_image_id().to_string().as_str()));
        writer
            .write_event(Event::Start(el))
            .map_err(|e| e.to_string())?;
        for meta in project.images() {
            writer
                .write_event(Event::Text(BytesText::from_escaped("\n    ")))
                .map_err(|e| e.to_string())?;
            let mut im = BytesStart::new("image");
            im.push_attribute(("id", meta.id.0.to_string().as_str()));
            im.push_attribute(("path", esc(&meta.path).as_str()));
            im.push_attribute(("width", meta.width.to_string().as_str()));
            im.push_attribute(("height", meta.height.to_string().as_str()));
            writer
                .write_event(Event::Empty(im))
                .map_err(|e| e.to_string())?;
        }
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n  ")))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::End(BytesStart::new("images").to_end()))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n  ")))
            .map_err(|e| e.to_string())?;
    }

    // ocr entries
    {
        let mut el = BytesStart::new("ocr");
        el.push_attribute(("next_id", project.ocr.next_id().to_string().as_str()));
        writer
            .write_event(Event::Start(el))
            .map_err(|e| e.to_string())?;
        for entry in project.ocr.entries() {
            writer
                .write_event(Event::Text(BytesText::from_escaped("\n    ")))
                .map_err(|e| e.to_string())?;
            let mut e = BytesStart::new("entry");
            e.push_attribute(("id", entry.id.0.to_string().as_str()));
            e.push_attribute(("image_id", entry.image_id.0.to_string().as_str()));
            let src = match entry.source {
                EntrySource::AutoOcr => "AutoOcr",
                EntrySource::Manual => "Manual",
            };
            e.push_attribute(("source", src));
            e.push_attribute(("score", entry.score.to_string().as_str()));
            e.push_attribute(("deleted", if entry.deleted { "true" } else { "false" }));
            writer
                .write_event(Event::Start(e))
                .map_err(|e| e.to_string())?;

            // text
            writer
                .write_event(Event::Text(BytesText::from_escaped("\n      ")))
                .map_err(|e| e.to_string())?;
            writer
                .write_event(Event::Start(BytesStart::new("text")))
                .map_err(|e| e.to_string())?;
            writer
                .write_event(Event::Text(BytesText::from_escaped(esc(&entry.text))))
                .map_err(|e| e.to_string())?;
            writer
                .write_event(Event::End(BytesStart::new("text").to_end()))
                .map_err(|e| e.to_string())?;

            // quad
            writer
                .write_event(Event::Text(BytesText::from_escaped("\n      ")))
                .map_err(|e| e.to_string())?;
            writer
                .write_event(Event::Start(BytesStart::new("quad")))
                .map_err(|e| e.to_string())?;
            for p in &entry.quad.points {
                writer
                    .write_event(Event::Text(BytesText::from_escaped("\n        ")))
                    .map_err(|e| e.to_string())?;
                let mut pt = BytesStart::new("point");
                pt.push_attribute(("x", p[0].to_string().as_str()));
                pt.push_attribute(("y", p[1].to_string().as_str()));
                writer
                    .write_event(Event::Empty(pt))
                    .map_err(|e| e.to_string())?;
            }
            writer
                .write_event(Event::Text(BytesText::from_escaped("\n      ")))
                .map_err(|e| e.to_string())?;
            writer
                .write_event(Event::End(BytesStart::new("quad").to_end()))
                .map_err(|e| e.to_string())?;

            writer
                .write_event(Event::Text(BytesText::from_escaped("\n    ")))
                .map_err(|e| e.to_string())?;
            writer
                .write_event(Event::End(BytesStart::new("entry").to_end()))
                .map_err(|e| e.to_string())?;
        }
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n  ")))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::End(BytesStart::new("ocr").to_end()))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n  ")))
            .map_err(|e| e.to_string())?;
    }

    // profiles
    {
        let mut el = BytesStart::new("profiles");
        el.push_attribute(("selected", project.profiles.selected_id().0.to_string().as_str()));
        el.push_attribute(("next_id", project.profiles.next_id().to_string().as_str()));
        writer
            .write_event(Event::Start(el))
            .map_err(|e| e.to_string())?;
        for prof in project.profiles.iter() {
            writer
                .write_event(Event::Text(BytesText::from_escaped("\n    ")))
                .map_err(|e| e.to_string())?;
            let mut p = BytesStart::new("profile");
            p.push_attribute(("id", prof.id.0.to_string().as_str()));
            p.push_attribute(("name", esc(&prof.name).as_str()));
            // if no deltas, use empty element? use start/end for consistency
            let has_deltas = !prof.deltas().is_empty();
            if has_deltas {
                writer
                    .write_event(Event::Start(p))
                    .map_err(|e| e.to_string())?;
                let mut deltas: Vec<_> = prof.deltas().iter().collect();
                deltas.sort_by_key(|(k, _)| k.0);
                for (eid, delta) in deltas {
                    if let Some(trans) = &delta.translation {
                        writer
                            .write_event(Event::Text(BytesText::from_escaped("\n      ")))
                            .map_err(|e| e.to_string())?;
                        let mut d = BytesStart::new("delta");
                        d.push_attribute(("entry_id", eid.0.to_string().as_str()));
                        writer
                            .write_event(Event::Start(d))
                            .map_err(|e| e.to_string())?;
                        writer
                            .write_event(Event::Text(BytesText::from_escaped("\n        ")))
                            .map_err(|e| e.to_string())?;
                        writer
                            .write_event(Event::Start(BytesStart::new("translation")))
                            .map_err(|e| e.to_string())?;
                        writer
                            .write_event(Event::Text(BytesText::from_escaped(esc(trans))))
                            .map_err(|e| e.to_string())?;
                        writer
                            .write_event(Event::End(BytesStart::new("translation").to_end()))
                            .map_err(|e| e.to_string())?;
                        writer
                            .write_event(Event::Text(BytesText::from_escaped("\n      ")))
                            .map_err(|e| e.to_string())?;
                        writer
                            .write_event(Event::End(BytesStart::new("delta").to_end()))
                            .map_err(|e| e.to_string())?;
                    }
                }
                writer
                    .write_event(Event::Text(BytesText::from_escaped("\n    ")))
                    .map_err(|e| e.to_string())?;
                writer
                    .write_event(Event::End(BytesStart::new("profile").to_end()))
                    .map_err(|e| e.to_string())?;
            } else {
                writer
                    .write_event(Event::Empty(p))
                    .map_err(|e| e.to_string())?;
            }
        }
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n  ")))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::End(BytesStart::new("profiles").to_end()))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n  ")))
            .map_err(|e| e.to_string())?;
    }

    // styles
    {
        let el = BytesStart::new("styles");
        writer
            .write_event(Event::Start(el))
            .map_err(|e| e.to_string())?;
        let mut styles: Vec<_> = project.styles().iter().collect();
        styles.sort_by_key(|(k, _)| k.0);
        for (eid, style) in styles {
            writer
                .write_event(Event::Text(BytesText::from_escaped("\n    ")))
                .map_err(|e| e.to_string())?;
            let mut s = BytesStart::new("style");
            s.push_attribute(("entry_id", eid.0.to_string().as_str()));
            s.push_attribute(("font_size", style.font_size.to_string().as_str()));
            s.push_attribute(("bold", if style.bold { "true" } else { "false" }));
            s.push_attribute(("italic", if style.italic { "true" } else { "false" }));
            s.push_attribute(("stroke_width", style.stroke_width.to_string().as_str()));
            s.push_attribute(("bg_radius", style.bg_radius.to_string().as_str()));
            s.push_attribute(("text_align", style.text_align.label()));
            s.push_attribute(("text_gradient", if style.text_gradient { "true" } else { "false" }));
            s.push_attribute(("gradient_dir", style.gradient_dir.label()));
            if let Some(fam) = &style.font_family {
                s.push_attribute(("font_family", esc(fam).as_str()));
            }
            writer
                .write_event(Event::Start(s))
                .map_err(|e| e.to_string())?;
            // colors
            for (tag, col) in [
                ("text_color", style.text_color),
                ("stroke_color", style.stroke_color),
                ("bg_color", style.bg_color),
                ("gradient_a", style.gradient_a),
                ("gradient_b", style.gradient_b),
            ] {
                writer
                    .write_event(Event::Text(BytesText::from_escaped("\n      ")))
                    .map_err(|e| e.to_string())?;
                let mut c = BytesStart::new(tag);
                c.push_attribute(("r", col[0].to_string().as_str()));
                c.push_attribute(("g", col[1].to_string().as_str()));
                c.push_attribute(("b", col[2].to_string().as_str()));
                c.push_attribute(("a", col[3].to_string().as_str()));
                writer
                    .write_event(Event::Empty(c))
                    .map_err(|e| e.to_string())?;
            }
            writer
                .write_event(Event::Text(BytesText::from_escaped("\n    ")))
                .map_err(|e| e.to_string())?;
            writer
                .write_event(Event::End(BytesStart::new("style").to_end()))
                .map_err(|e| e.to_string())?;
        }
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n  ")))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::End(BytesStart::new("styles").to_end()))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n  ")))
            .map_err(|e| e.to_string())?;
    }

    // view_quads
    {
        let el = BytesStart::new("view_quads");
        writer
            .write_event(Event::Start(el))
            .map_err(|e| e.to_string())?;
        let mut vqs: Vec<_> = project.view_quads().iter().collect();
        vqs.sort_by_key(|(k, _)| k.0);
        for (eid, quad) in vqs {
            writer
                .write_event(Event::Text(BytesText::from_escaped("\n    ")))
                .map_err(|e| e.to_string())?;
            let mut q = BytesStart::new("view_quad");
            q.push_attribute(("entry_id", eid.0.to_string().as_str()));
            writer
                .write_event(Event::Start(q))
                .map_err(|e| e.to_string())?;
            for p in &quad.points {
                writer
                    .write_event(Event::Text(BytesText::from_escaped("\n      ")))
                    .map_err(|e| e.to_string())?;
                let mut pt = BytesStart::new("point");
                pt.push_attribute(("x", p[0].to_string().as_str()));
                pt.push_attribute(("y", p[1].to_string().as_str()));
                writer
                    .write_event(Event::Empty(pt))
                    .map_err(|e| e.to_string())?;
            }
            writer
                .write_event(Event::Text(BytesText::from_escaped("\n    ")))
                .map_err(|e| e.to_string())?;
            writer
                .write_event(Event::End(BytesStart::new("view_quad").to_end()))
                .map_err(|e| e.to_string())?;
        }
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n  ")))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::End(BytesStart::new("view_quads").to_end()))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n  ")))
            .map_err(|e| e.to_string())?;
    }

    // extras
    {
        let el = BytesStart::new("extras");
        writer
            .write_event(Event::Start(el))
            .map_err(|e| e.to_string())?;
        // notes
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n    ")))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::Start(BytesStart::new("notes")))
            .map_err(|e| e.to_string())?;
        let mut notes: Vec<_> = project.extras.notes.iter().collect();
        notes.sort_by_key(|(k, _)| k.0);
        for (eid, note) in notes {
            writer
                .write_event(Event::Text(BytesText::from_escaped("\n      ")))
                .map_err(|e| e.to_string())?;
            let mut n = BytesStart::new("note");
            n.push_attribute(("entry_id", eid.0.to_string().as_str()));
            writer
                .write_event(Event::Start(n))
                .map_err(|e| e.to_string())?;
            writer
                .write_event(Event::Text(BytesText::from_escaped(esc(note))))
                .map_err(|e| e.to_string())?;
            writer
                .write_event(Event::End(BytesStart::new("note").to_end()))
                .map_err(|e| e.to_string())?;
        }
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n    ")))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::End(BytesStart::new("notes").to_end()))
            .map_err(|e| e.to_string())?;

        // inpaint_patches
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n    ")))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::Start(BytesStart::new("inpaint_patches")))
            .map_err(|e| e.to_string())?;
        for patch in &project.extras.inpaint_patches {
            writer
                .write_event(Event::Text(BytesText::from_escaped("\n      ")))
                .map_err(|e| e.to_string())?;
            let mut p = BytesStart::new("patch");
            p.push_attribute(("id", patch.id.0.to_string().as_str()));
            p.push_attribute(("image_id", patch.image_id.0.to_string().as_str()));
            p.push_attribute(("x", patch.bounds[0].to_string().as_str()));
            p.push_attribute(("y", patch.bounds[1].to_string().as_str()));
            p.push_attribute(("w", patch.bounds[2].to_string().as_str()));
            p.push_attribute(("h", patch.bounds[3].to_string().as_str()));
            if let Some(quad) = patch.quad {
                writer
                    .write_event(Event::Start(p))
                    .map_err(|e| e.to_string())?;
                writer
                    .write_event(Event::Text(BytesText::from_escaped("\n        ")))
                    .map_err(|e| e.to_string())?;
                writer
                    .write_event(Event::Start(BytesStart::new("quad")))
                    .map_err(|e| e.to_string())?;
                for pt in &quad.points {
                    writer
                        .write_event(Event::Text(BytesText::from_escaped("\n          ")))
                        .map_err(|e| e.to_string())?;
                    let mut pt_el = BytesStart::new("point");
                    pt_el.push_attribute(("x", pt[0].to_string().as_str()));
                    pt_el.push_attribute(("y", pt[1].to_string().as_str()));
                    writer
                        .write_event(Event::Empty(pt_el))
                        .map_err(|e| e.to_string())?;
                }
                writer
                    .write_event(Event::Text(BytesText::from_escaped("\n        ")))
                    .map_err(|e| e.to_string())?;
                writer
                    .write_event(Event::End(BytesStart::new("quad").to_end()))
                    .map_err(|e| e.to_string())?;
                writer
                    .write_event(Event::Text(BytesText::from_escaped("\n      ")))
                    .map_err(|e| e.to_string())?;
                writer
                    .write_event(Event::End(BytesStart::new("patch").to_end()))
                    .map_err(|e| e.to_string())?;
            } else {
                writer
                    .write_event(Event::Empty(p))
                    .map_err(|e| e.to_string())?;
            }
        }
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n    ")))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::End(BytesStart::new("inpaint_patches").to_end()))
            .map_err(|e| e.to_string())?;

        // shapes
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n    ")))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::Start(BytesStart::new("shapes")))
            .map_err(|e| e.to_string())?;
        for shape in &project.extras.shapes {
            writer
                .write_event(Event::Text(BytesText::from_escaped("\n      ")))
                .map_err(|e| e.to_string())?;
            let mut s = BytesStart::new("shape");
            let kind = match shape.kind {
                ShapeKind::Rect => "Rect",
                ShapeKind::Polygon => "Polygon",
            };
            s.push_attribute(("kind", kind));
            writer
                .write_event(Event::Start(s))
                .map_err(|e| e.to_string())?;
            for pt in &shape.points {
                writer
                    .write_event(Event::Text(BytesText::from_escaped("\n        ")))
                    .map_err(|e| e.to_string())?;
                let mut pp = BytesStart::new("point");
                pp.push_attribute(("x", pt[0].to_string().as_str()));
                pp.push_attribute(("y", pt[1].to_string().as_str()));
                writer
                    .write_event(Event::Empty(pp))
                    .map_err(|e| e.to_string())?;
            }
            writer
                .write_event(Event::Text(BytesText::from_escaped("\n      ")))
                .map_err(|e| e.to_string())?;
            writer
                .write_event(Event::End(BytesStart::new("shape").to_end()))
                .map_err(|e| e.to_string())?;
        }
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n    ")))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::End(BytesStart::new("shapes").to_end()))
            .map_err(|e| e.to_string())?;

        writer
            .write_event(Event::Text(BytesText::from_escaped("\n  ")))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::End(BytesStart::new("extras").to_end()))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::Text(BytesText::from_escaped("\n")))
            .map_err(|e| e.to_string())?;
    }

    writer
        .write_event(Event::End(BytesStart::new("project").to_end()))
        .map_err(|e| e.to_string())?;
    writer
        .write_event(Event::Text(BytesText::from_escaped("\n")))
        .map_err(|e| e.to_string())?;

    let bytes = writer.into_inner();
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Deserialization — manual Reader loop for robustness
// ---------------------------------------------------------------------------

struct ParseCtx {
    images: Vec<ImageMeta>,
    next_image_id: u64,
    ocr_entries: Vec<OcrEntry>,
    ocr_next_id: u64,
    profiles: Vec<Profile>,
    selected: ProfileId,
    profiles_next_id: u64,
    styles: HashMap<EntryId, EntryStyle>,
    view_quads: HashMap<EntryId, Quad>,
    notes: HashMap<EntryId, String>,
    inpaint_patches: Vec<InpaintPatch>,
    shapes: Vec<Shape>,
}

impl Default for ParseCtx {
    fn default() -> Self {
        Self {
            images: Vec::new(),
            next_image_id: 0,
            ocr_entries: Vec::new(),
            ocr_next_id: 0,
            profiles: Vec::new(),
            selected: ProfileId(0),
            profiles_next_id: 0,
            styles: HashMap::new(),
            view_quads: HashMap::new(),
            notes: HashMap::new(),
            inpaint_patches: Vec::new(),
            shapes: Vec::new(),
        }
    }
}

pub fn from_xml_str(s: &str) -> Result<Project, String> {
    let mut reader = Reader::from_str(s);
    reader.config_mut().trim_text(true);
    let mut ctx = ParseCtx::default();
    // temporary holders for nested parsing
    let mut buf = Vec::new();
    // we parse via event loop
    // stack to track current element path
    let mut stack: Vec<String> = Vec::new();
    // current entry being built
    let mut cur_entry: Option<OcrEntry> = None;
    let mut cur_quad_points: Vec<[f32; 2]> = Vec::new();
    let mut cur_profile: Option<Profile> = None;
    let mut cur_delta_entry: Option<EntryId> = None;
    let mut cur_delta_trans: Option<String> = None;
    let mut cur_style: Option<(EntryId, EntryStyle)> = None;
    let mut cur_style_colors: HashMap<String, [u8;4]> = HashMap::new();
    let mut cur_view_quad: Option<(EntryId, Quad)> = None;
    let mut cur_view_points: Vec<[f32;2]> = Vec::new();
    let mut cur_note: Option<(EntryId, String)> = None;
    let mut cur_shape: Option<Shape> = None;
    let mut cur_shape_points: Vec<[f32;2]> = Vec::new();
    let mut cur_inpaint_patch: Option<InpaintPatch> = None;
    let mut cur_inpaint_quad_points: Vec<[f32; 2]> = Vec::new();
    // accumulate text content
    let mut text_buf = String::new();
    let mut collecting: Option<String> = None; // tag name whose text we collect

    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => return Err(format!("xml read error: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                stack.push(name.clone());
                match name.as_str() {
                    "project" => {
                        if let Some(v) = attr(&e, b"version") {
                            let _ = v; // ignore version for now
                        }
                    }
                    "images" => {
                        ctx.next_image_id = attr(&e, b"next_image_id").map(|v| parse_u64(&v)).unwrap_or(0);
                    }
                    "image" => {
                        // empty or start
                        let id = attr(&e, b"id").map(|v| ImageId(parse_u64(&v))).unwrap_or(ImageId(0));
                        let path = attr(&e, b"path").map(|v| unesc(&v)).unwrap_or_default();
                        let width = attr(&e, b"width").map(|v| parse_f32(&v)).unwrap_or(0.0);
                        let height = attr(&e, b"height").map(|v| parse_f32(&v)).unwrap_or(0.0);
                        // this is Empty in writer, but if Start we will close on End
                        // To handle both, push immediately; if it's Start we won't duplicate because we treat End as no-op for image
                        // Detect if empty: we are in Start event, but empty was handled as Empty earlier, so this branch only for non-empty (unlikely)
                        ctx.images.push(ImageMeta { id, path, width, height });
                    }
                    "ocr" => {
                        ctx.ocr_next_id = attr(&e, b"next_id").map(|v| parse_u64(&v)).unwrap_or(0);
                    }
                    "entry" => {
                        let id = attr(&e, b"id").map(|v| EntryId(parse_u64(&v))).unwrap_or(EntryId(0));
                        let image_id = attr(&e, b"image_id").map(|v| ImageId(parse_u64(&v))).unwrap_or(ImageId(0));
                        let source = match attr(&e, b"source").as_deref() {
                            Some("Manual") => EntrySource::Manual,
                            _ => EntrySource::AutoOcr,
                        };
                        let score = attr(&e, b"score").map(|v| parse_f32(&v)).unwrap_or(0.0);
                        let deleted = attr(&e, b"deleted").map(|v| v=="true").unwrap_or(false);
                        cur_entry = Some(OcrEntry { id, image_id, source, text: String::new(), score, quad: Quad{points:[[0.0;2];4]}, deleted });
                        cur_quad_points.clear();
                    }
                    "quad" => {
                        // If this quad belongs to an inpaint patch, handle separately
                        let parent_is_patch = stack.len() >= 2 && stack[stack.len() - 2] == "patch" && cur_inpaint_patch.is_some();
                        if parent_is_patch {
                            cur_inpaint_quad_points.clear();
                        } else {
                            cur_quad_points.clear();
                            cur_view_points.clear();
                        }
                        // distinguish entry quad vs view_quad: based on stack parent
                        // we'll just collect points; assignment on End
                    }
                    "point" => {
                        let x = attr(&e, b"x").map(|v| parse_f32(&v)).unwrap_or(0.0);
                        let y = attr(&e, b"y").map(|v| parse_f32(&v)).unwrap_or(0.0);
                        // determine where to push: if inside entry's quad or view_quad or shape or patch quad
                        if stack.len() >= 2 {
                            let parent = &stack[stack.len()-2];
                            if parent=="quad" {
                                // check if this quad belongs to an inpaint patch
                                let is_patch_quad = stack.len() >= 3 && stack[stack.len()-3] == "patch" && cur_inpaint_patch.is_some();
                                if is_patch_quad {
                                    cur_inpaint_quad_points.push([x,y]);
                                } else if stack.iter().any(|s| s=="view_quads") && cur_view_quad.is_some() {
                                    cur_view_points.push([x,y]);
                                } else if cur_entry.is_some() && cur_shape.is_none() {
                                    cur_quad_points.push([x,y]);
                                } else if cur_shape.is_some() {
                                    cur_shape_points.push([x,y]);
                                }
                            } else if parent=="view_quad" {
                                cur_view_points.push([x,y]);
                            } else if parent=="shape" {
                                cur_shape_points.push([x,y]);
                            }
                        }
                        // empty element, no End
                        // pop it immediately because it's empty
                        stack.pop();
                    }
                    "text" | "translation" => {
                        collecting = Some(name.clone());
                        text_buf.clear();
                    }
                    "profiles" => {
                        ctx.selected = attr(&e, b"selected").map(|v| ProfileId(parse_u64(&v))).unwrap_or(ProfileId(0));
                        ctx.profiles_next_id = attr(&e, b"next_id").map(|v| parse_u64(&v)).unwrap_or(1);
                    }
                    "profile" => {
                        let id = attr(&e, b"id").map(|v| ProfileId(parse_u64(&v))).unwrap_or(ProfileId(0));
                        let name = attr(&e, b"name").map(|v| unesc(&v)).unwrap_or_default();
                        cur_profile = Some(Profile::from_raw(id, name, HashMap::new()));
                    }
                    "delta" => {
                        let eid = attr(&e, b"entry_id").map(|v| EntryId(parse_u64(&v))).unwrap_or(EntryId(0));
                        cur_delta_entry = Some(eid);
                        cur_delta_trans = None;
                    }
                    "style" => {
                        let eid = attr(&e, b"entry_id").map(|v| EntryId(parse_u64(&v))).unwrap_or(EntryId(0));
                        let font_size = attr(&e, b"font_size").map(|v| parse_f32(&v)).unwrap_or(14.0);
                        let bold = attr(&e, b"bold").map(|v| v=="true").unwrap_or(false);
                        let italic = attr(&e, b"italic").map(|v| v=="true").unwrap_or(false);
                        let stroke_width = attr(&e, b"stroke_width").map(|v| parse_f32(&v)).unwrap_or(0.0);
                        let bg_radius = attr(&e, b"bg_radius").map(|v| parse_f32(&v)).unwrap_or(0.0);
                        let text_align = attr(&e, b"text_align").map(|v| TextAlign::from_label(&v)).unwrap_or(TextAlign::Circular);
                        let text_gradient = attr(&e, b"text_gradient").map(|v| v=="true").unwrap_or(false);
                        let gradient_dir = attr(&e, b"gradient_dir").map(|v| TextGradientDir::from_label(&v)).unwrap_or(TextGradientDir::TopToBottom);
                        let font_family = attr(&e, b"font_family").map(|v| unesc(&v));
                        cur_style = Some((eid, EntryStyle{ font_size, bold, italic, text_color:[0,0,0,255], stroke_color:[0,0,0,255], stroke_width, bg_color:[255,255,255,255], bg_radius, font_family, text_align, text_gradient, gradient_a:[0,0,0,255], gradient_b:[0,0,0,255], gradient_dir }));
                        cur_style_colors.clear();
                    }
                    "text_color" | "stroke_color" | "bg_color" | "gradient_a" | "gradient_b" => {
                        let r = attr(&e, b"r").map(|v| parse_u8(&v)).unwrap_or(0);
                        let g = attr(&e, b"g").map(|v| parse_u8(&v)).unwrap_or(0);
                        let b = attr(&e, b"b").map(|v| parse_u8(&v)).unwrap_or(0);
                        let a = attr(&e, b"a").map(|v| parse_u8(&v)).unwrap_or(255);
                        cur_style_colors.insert(name.clone(), [r,g,b,a]);
                        // empty, pop
                        stack.pop();
                    }
                    "view_quads" => {},
                    "view_quad" => {
                        let eid = attr(&e, b"entry_id").map(|v| EntryId(parse_u64(&v))).unwrap_or(EntryId(0));
                        cur_view_quad = Some((eid, Quad{points:[[0.0;2];4]}));
                        cur_view_points.clear();
                    }
                    "extras" => {},
                    "notes" => {},
                    "note" => {
                        let eid = attr(&e, b"entry_id").map(|v| EntryId(parse_u64(&v))).unwrap_or(EntryId(0));
                        cur_note = Some((eid, String::new()));
                        collecting = Some("note".to_string());
                        text_buf.clear();
                    }
                    "inpaint_patches" => {},
                    "patch" => {
                        // could be inpaint patch: distinguish by parent
                        let parent = stack.get(stack.len().saturating_sub(2)).map(|s| s.as_str()).unwrap_or("");
                        if parent=="inpaint_patches" {
                            let id = attr(&e, b"id").map(|v| InpaintId(parse_u64(&v))).unwrap_or(InpaintId(ctx.inpaint_patches.len() as u64));
                            let image_id = attr(&e, b"image_id").map(|v| ImageId(parse_u64(&v))).unwrap_or(ImageId(0));
                            let x = attr(&e, b"x").map(|v| parse_f32(&v)).unwrap_or(0.0);
                            let y = attr(&e, b"y").map(|v| parse_f32(&v)).unwrap_or(0.0);
                            let w = attr(&e, b"w").map(|v| parse_f32(&v)).unwrap_or(0.0);
                            let h = attr(&e, b"h").map(|v| parse_f32(&v)).unwrap_or(0.0);
                            // New format with nested <quad> will be handled on End; defer push.
                            // To detect empty vs container, we peek if next event is quad start — but in Start event we can't know.
                            // Heuristic: store pending and only push on End. For backward compat, if this Start has no children,
                            // it will be closed via Empty event, not Start. This Start branch only for new container patches.
                            cur_inpaint_patch = Some(InpaintPatch{ id, image_id, bounds:[x,y,w,h], quad: None});
                            cur_inpaint_quad_points.clear();
                            // Do NOT pop stack here — keep patch on stack for child quad
                        }
                    }
                    "shapes" => {},
                    "shape" => {
                        let kind = match attr(&e, b"kind").as_deref() {
                            Some("Polygon") => ShapeKind::Polygon,
                            _ => ShapeKind::Rect,
                        };
                        cur_shape = Some(Shape{ kind, points: Vec::new() });
                        cur_shape_points.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                // handle empty elements that were not handled in Start
                match name.as_str() {
                    "image" => {
                        let id = attr(&e, b"id").map(|v| ImageId(parse_u64(&v))).unwrap_or(ImageId(0));
                        let path = attr(&e, b"path").map(|v| unesc(&v)).unwrap_or_default();
                        let width = attr(&e, b"width").map(|v| parse_f32(&v)).unwrap_or(0.0);
                        let height = attr(&e, b"height").map(|v| parse_f32(&v)).unwrap_or(0.0);
                        ctx.images.push(ImageMeta { id, path, width, height });
                    }
                    "point" => {
                        // handled above via Start, but Empty also occurs for point in some contexts if we pushed and popped differently
                        // Need to handle if not already handled: check stack parent for point
                        let x = attr(&e, b"x").map(|v| parse_f32(&v)).unwrap_or(0.0);
                        let y = attr(&e, b"y").map(|v| parse_f32(&v)).unwrap_or(0.0);
                        // infer parent from current stack top (since Empty doesn't push)
                        if let Some(parent) = stack.last().map(|s| s.as_str()) {
                            if parent=="quad" {
                                // check if this quad belongs to a patch
                                let is_patch_quad = cur_inpaint_patch.is_some() && stack.len() >= 1 && stack.iter().any(|s| s=="patch");
                                // More precise: if stack contains patch and quad parent is patch quad
                                if is_patch_quad && cur_inpaint_patch.is_some() {
                                    // Heuristic: if we are inside inpaint_patches/patch/quad, push to patch quad points
                                    // Detect by checking if top of stack before quad was patch
                                    // Since stack is [..., inpaint_patches, patch, quad], the presence of patch indicates patch quad
                                    cur_inpaint_quad_points.push([x,y]);
                                } else if stack.iter().any(|s| s=="view_quads") && cur_view_quad.is_some() {
                                    cur_view_points.push([x,y]);
                                } else if cur_entry.is_some() && cur_shape.is_none() {
                                    cur_quad_points.push([x,y]);
                                } else if cur_shape.is_some() {
                                    cur_shape_points.push([x,y]);
                                } else if cur_inpaint_patch.is_some() {
                                    cur_inpaint_quad_points.push([x,y]);
                                }
                            } else if parent=="view_quad" {
                                cur_view_points.push([x,y]);
                            } else if parent=="shape" {
                                cur_shape_points.push([x,y]);
                            }
                        } else {
                            // fallback: if cur_entry exists push there
                            if cur_entry.is_some() {
                                cur_quad_points.push([x,y]);
                            } else if cur_inpaint_patch.is_some() {
                                cur_inpaint_quad_points.push([x,y]);
                            }
                        }
                    }
                    "text_color" | "stroke_color" | "bg_color" | "gradient_a" | "gradient_b" => {
                        let r = attr(&e, b"r").map(|v| parse_u8(&v)).unwrap_or(0);
                        let g = attr(&e, b"g").map(|v| parse_u8(&v)).unwrap_or(0);
                        let b = attr(&e, b"b").map(|v| parse_u8(&v)).unwrap_or(0);
                        let a = attr(&e, b"a").map(|v| parse_u8(&v)).unwrap_or(255);
                        cur_style_colors.insert(name.clone(), [r,g,b,a]);
                    }
                    "patch" => {
                        let parent = stack.last().map(|s| s.as_str()).unwrap_or("");
                        if parent=="inpaint_patches" {
                            let id = attr(&e, b"id").map(|v| InpaintId(parse_u64(&v))).unwrap_or(InpaintId(ctx.inpaint_patches.len() as u64));
                            let image_id = attr(&e, b"image_id").map(|v| ImageId(parse_u64(&v))).unwrap_or(ImageId(0));
                            let x = attr(&e, b"x").map(|v| parse_f32(&v)).unwrap_or(0.0);
                            let y = attr(&e, b"y").map(|v| parse_f32(&v)).unwrap_or(0.0);
                            let w = attr(&e, b"w").map(|v| parse_f32(&v)).unwrap_or(0.0);
                            let h = attr(&e, b"h").map(|v| parse_f32(&v)).unwrap_or(0.0);
                            ctx.inpaint_patches.push(InpaintPatch{ id, image_id, bounds:[x,y,w,h], quad: None});
                        }
                    }
                    "profile" => {
                        // empty profile with no deltas
                        let id = attr(&e, b"id").map(|v| ProfileId(parse_u64(&v))).unwrap_or(ProfileId(0));
                        let n = attr(&e, b"name").map(|v| unesc(&v)).unwrap_or_default();
                        ctx.profiles.push(Profile::from_raw(id, n, HashMap::new()));
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                if collecting.is_some() {
                    if let Ok(txt) = e.unescape() {
                        text_buf.push_str(&txt);
                    }
                }
            }
            Ok(Event::CData(e)) => {
                if collecting.is_some() {
                    if let Ok(txt) = std::str::from_utf8(&e.into_inner()) {
                        text_buf.push_str(txt);
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                // handle closing for collecting tags
                match name.as_str() {
                    "text" => {
                        if let Some(entry) = cur_entry.as_mut() {
                            if collecting.as_deref()==Some("text") {
                                entry.text = unesc(&text_buf);
                                collecting = None;
                                text_buf.clear();
                            }
                        }
                    }
                    "translation" => {
                        if collecting.as_deref()==Some("translation") {
                            cur_delta_trans = Some(unesc(&text_buf));
                            collecting = None;
                            text_buf.clear();
                        }
                    }
                    "note" => {
                        if let Some((eid, _)) = cur_note.take() {
                            let note = unesc(&text_buf);
                            ctx.notes.insert(eid, note);
                            collecting = None;
                            text_buf.clear();
                        }
                    }
                    "quad" => {
                        // If this quad belongs to an inpaint patch, assign to pending patch
                        let is_patch_quad = cur_inpaint_patch.is_some() && stack.len() >= 2 && stack[stack.len() - 2] == "patch";
                        if is_patch_quad {
                            if cur_inpaint_quad_points.len() == 4 {
                                if let Some(p) = cur_inpaint_patch.as_mut() {
                                    p.quad = Some(Quad{ points: [cur_inpaint_quad_points[0], cur_inpaint_quad_points[1], cur_inpaint_quad_points[2], cur_inpaint_quad_points[3]] });
                                }
                            }
                            cur_inpaint_quad_points.clear();
                        } else if let Some(entry) = cur_entry.as_mut() {
                            if cur_quad_points.len() == 4 {
                                entry.quad.points = [cur_quad_points[0], cur_quad_points[1], cur_quad_points[2], cur_quad_points[3]];
                            }
                            cur_quad_points.clear();
                        } else {
                            cur_quad_points.clear();
                        }
                    }
                    "entry" => {
                        if let Some(entry) = cur_entry.take() {
                            ctx.ocr_entries.push(entry);
                        }
                    }
                    "delta" => {
                        if let Some(eid) = cur_delta_entry.take() {
                            let trans = cur_delta_trans.take();
                            // need to add to current profile's deltas
                            if let Some(prof) = cur_profile.as_mut() {
                                // directly insert delta
                                prof.deltas().len(); // just to avoid unused
                                // we need mutable access to deltas: use from_raw reconstruction? Instead use set_translation via public API after creation
                                // For now, stash in temporary map and later reconstruct profile
                                // Simpler: keep a temporary HashMap for cur_profile
                                // But our Profile::from_raw allows us to mutate via building new HashMap
                                // We'll accumulate in a separate map
                                // workaround: we store cur deltas in ctx? Instead we maintain cur_profile_deltas map
                                // We need to handle this: we didn't have a per-profile delta map.
                                // We'll insert directly via a hack: create new Profile with updated deltas
                                let mut deltas = prof.deltas().clone();
                                if let Some(t) = trans {
                                    deltas.insert(eid, easyscanlate_model::profile::EntryDelta{ translation: Some(t)});
                                } else {
                                    deltas.insert(eid, easyscanlate_model::profile::EntryDelta{ translation: None});
                                }
                                // reconstruct profile
                                let new_prof = Profile::from_raw(prof.id, prof.name.clone(), deltas);
                                *prof = new_prof;
                            }
                        }
                    }
                    "profile" => {
                        if let Some(prof) = cur_profile.take() {
                            ctx.profiles.push(prof);
                        }
                    }
                    "style" => {
                        if let Some((eid, mut style)) = cur_style.take() {
                            if let Some(c) = cur_style_colors.get("text_color") { style.text_color = *c; }
                            if let Some(c) = cur_style_colors.get("stroke_color") { style.stroke_color = *c; }
                            if let Some(c) = cur_style_colors.get("bg_color") { style.bg_color = *c; }
                            if let Some(c) = cur_style_colors.get("gradient_a") { style.gradient_a = *c; }
                            if let Some(c) = cur_style_colors.get("gradient_b") { style.gradient_b = *c; }
                            ctx.styles.insert(eid, style);
                            cur_style_colors.clear();
                        }
                    }
                    "view_quad" => {
                        if let Some((eid, _)) = cur_view_quad.take() {
                            let quad = if cur_view_points.len()==4 {
                                Quad{points:[cur_view_points[0], cur_view_points[1], cur_view_points[2], cur_view_points[3]]}
                            } else { Quad{points:[[0.0;2];4]} };
                            ctx.view_quads.insert(eid, quad);
                            cur_view_points.clear();
                        }
                    }
                    "shape" => {
                        if let Some(mut shape) = cur_shape.take() {
                            shape.points = cur_shape_points.clone();
                            ctx.shapes.push(shape);
                            cur_shape_points.clear();
                        }
                    }
                    "patch" => {
                        if let Some(patch) = cur_inpaint_patch.take() {
                            ctx.inpaint_patches.push(patch);
                            cur_inpaint_quad_points.clear();
                        }
                    }
                    _ => {}
                }
                // pop stack
                if let Some(top) = stack.last() {
                    if top == &name {
                        stack.pop();
                    }
                }
                if collecting.as_deref() == Some(name.as_str()) {
                    collecting = None;
                    text_buf.clear();
                }
            }
            _ => {}
        }
        buf.clear();
    }

    // reconstruct Profiles if empty? Default has one profile
    let profiles = if ctx.profiles.is_empty() {
        easyscanlate_model::Profiles::default()
    } else {
        // ensure selected exists
        let selected = if ctx.profiles.iter().any(|p| p.id==ctx.selected) { ctx.selected } else { ctx.profiles[0].id };
        let next = if ctx.profiles_next_id==0 {
            ctx.profiles.iter().map(|p| p.id.0+1).max().unwrap_or(1)
        } else { ctx.profiles_next_id };
        easyscanlate_model::Profiles::from_raw(ctx.profiles, selected, next)
    };

    let ocr_next = if ctx.ocr_next_id==0 {
        ctx.ocr_entries.iter().map(|e| e.id.0+1).max().unwrap_or(0)
    } else { ctx.ocr_next_id };
    let ocr = OcrResult::from_raw(ctx.ocr_entries, ocr_next);

    let next_image_id = if ctx.next_image_id==0 {
        ctx.images.iter().map(|m| m.id.0+1).max().unwrap_or(0)
    } else { ctx.next_image_id };

    let extras = Extras { notes: ctx.notes, inpaint_patches: ctx.inpaint_patches, shapes: ctx.shapes };

    let project = Project::from_raw(ctx.images, next_image_id, ocr, profiles, ctx.styles, ctx.view_quads, extras);
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;
    use easyscanlate_model::{EntrySource, ImageId, NewEntry, Quad};

    #[test]
    fn roundtrip_basic() {
        let mut project = Project::new();
        let img = project.add_image("a.jpg", 100.0, 200.0);
        let entry = project.ocr.append_for_image(img, NewEntry{ source: EntrySource::AutoOcr, text:"hello".into(), score:0.9, quad: Quad{points:[[0.0,0.0],[10.0,0.0],[10.0,10.0],[0.0,10.0]]}});
        project.profiles.selected_mut().set_translation(entry, Some("hi".into()));
        let style = EntryStyle { bold:true, ..EntryStyle::default()};
        project.set_entry_style(entry, style);
        project.set_view_quad(entry, Quad{points:[[1.0,1.0],[11.0,1.0],[11.0,11.0],[1.0,11.0]]});
        project.extras.set_note(entry, "note".into());
        let xml = to_xml_string(&project).unwrap();
        let back = from_xml_str(&xml).unwrap();
        assert_eq!(back.images().len(), 1);
        assert_eq!(back.ocr.entries().len(), 1);
        assert_eq!(back.ocr.get(entry).unwrap().text, "hello");
        assert_eq!(back.profiles.selected().translation_of(entry), Some("hi"));
        assert_eq!(back.entry_style(entry).bold, true);
        assert_eq!(back.view_quads().get(&entry).is_some(), true);
        assert_eq!(back.extras.note(entry), Some("note"));
    }

    #[test]
    fn roundtrip_empty() {
        let project = Project::new();
        let xml = to_xml_string(&project).unwrap();
        let back = from_xml_str(&xml).unwrap();
        assert_eq!(back.images().len(), 0);
        assert_eq!(back.ocr.entries().len(), 0);
    }

    #[test]
    fn roundtrip_inpaint_quad() {
        let mut project = Project::new();
        let img = project.add_image("a.jpg", 100.0, 100.0);
        let quad = Quad { points: [[10.0,20.0],[80.0,0.0],[90.0,30.0],[20.0,50.0]] };
        let ev = project.add_inpaint_patch_with_quad(img, quad);
        assert!(matches!(ev, easyscanlate_model::ModelEvent::InpaintAdded { quad: Some(_), .. }));
        let xml = to_xml_string(&project).unwrap();
        assert!(xml.contains("<quad>"), "xml should contain quad for inpaint patch");
        let back = from_xml_str(&xml).unwrap();
        assert_eq!(back.extras.inpaint_patches.len(), 1);
        let patch = &back.extras.inpaint_patches[0];
        assert!(patch.quad.is_some(), "quad should be preserved");
        let q = patch.quad.unwrap();
        assert_eq!(q.points[0], [10.0,20.0]);
        assert_eq!(q.points[2], [90.0,30.0]);
        // legacy compat: loading old xml without quad should give None
        let legacy_xml = r#"<?xml version="1.0" encoding="UTF-8"?><project version="1"><images next_image_id="1"><image id="0" path="a.jpg" width="100" height="100"/></images><ocr next_id="0"></ocr><profiles selected="0" next_id="1"><profile id="0" name="Default"/></profiles><styles></styles><view_quads></view_quads><extras><notes></notes><inpaint_patches><patch id="0" image_id="0" x="10" y="20" w="70" h="30"/></inpaint_patches><shapes></shapes></extras></project>"#;
        let legacy = from_xml_str(legacy_xml).unwrap();
        assert_eq!(legacy.extras.inpaint_patches[0].quad, None);
    }
}
