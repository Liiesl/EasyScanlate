//! Legacy ManhwaOCR JSON .mmtl import/export.
//! Mirrors `ManhwaOCR/app/core/project_model.py` and `master.json`/`meta.json`.
//! For interoperability: import legacy zips, export to legacy for ManhwaOCR.

use std::collections::HashMap;

use easyscanlate_model::{
    natural_cmp, EntrySource, ImageId, NewEntry, OcrResult, ProfileId, Project, Quad,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct LegacyMeta {
    #[serde(default)]
    original_language: Option<String>,
    #[serde(default)]
    active_profile_name: Option<String>,
    #[serde(default)]
    created: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LegacyEntry {
    row_number: serde_json::Value, // int or float or string
    filename: String,
    coordinates: [[f32; 2]; 4],
    text: String,
    #[serde(default)]
    confidence: Option<f32>,
    #[serde(default)]
    translations: Option<HashMap<String, String>>,
    #[serde(default)]
    custom_style: Option<serde_json::Value>,
    #[serde(default)]
    is_deleted: Option<bool>,
    #[serde(default)]
    is_manual: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LegacyInpaintRecord {
    id: String,
    target_image: String,
    coordinates: [f32; 4],
    patch_filename: String,
}

// Build Project from legacy JSON data (in-memory).
// Callers provide parsed master entries + meta, plus image dimensions map (filename -> (w,h)) if available.
// For import from ZIP we read image file names directly.
pub fn project_from_legacy(
    master: &[LegacyEntry],
    meta: &LegacyMeta,
    image_dims: &HashMap<String, (f32, f32)>,
) -> Result<Project, String> {
    let mut project = Project::new();
    // filename -> ImageId
    let mut file_to_id: HashMap<String, ImageId> = HashMap::new();
    // collect unique filenames in sorted order to keep deterministic ids
    let mut filenames: Vec<String> = master.iter().map(|e| e.filename.clone()).collect();
    // also include images that have no entries? caller should add them separately; we handle only those present
    filenames.sort_by(|a, b| natural_cmp(a, b));
    filenames.dedup();
    for fname in &filenames {
        let (w, h) = image_dims.get(fname).copied().unwrap_or((1000.0, 1000.0));
        let id = project.add_image(fname.clone(), w, h);
        file_to_id.insert(fname.clone(), id);
    }

    // need to map row_number -> EntryId but EntryId is generated sequentially; we will respect row_number as ordering but generate fresh ids
    // We also need to keep next_id based on max row_number
    // We'll sort by row_number float value to match ManhwaOCR sorting
    let mut entries_sorted: Vec<&LegacyEntry> = master.iter().collect();
    entries_sorted.sort_by(|a, b| {
        let fa = row_to_f64(&a.row_number).unwrap_or(0.0);
        let fb = row_to_f64(&b.row_number).unwrap_or(0.0);
        fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
    });

    // temporary map old row_number (u64) -> new EntryId for translation linking if needed? No, translations per entry directly.
    // Collect profiles set
    let mut profile_names: HashMap<String, ProfileId> = HashMap::new();
    // Ensure Default exists already (id 0)
    // For each entry, gather translations keys
    for le in &entries_sorted {
        if let Some(trans) = &le.translations {
            for k in trans.keys() {
                if k != "Original" && project.profiles.find_by_name(k).is_none() && !profile_names.contains_key(k) {
                    let id = project.profiles.add(k.clone());
                    profile_names.insert(k.clone(), id);
                }
            }
        }
    }
    // also ensure active_profile exists
    if let Some(active) = &meta.active_profile_name
        && project.profiles.find_by_name(active).is_none() {
            let _ = project.profiles.add(active.clone());
        }

    // Now append entries
    // We'll need to map each legacy entry to an OcrEntry and insert translations after
    for le in entries_sorted {
        let image_id = match file_to_id.get(&le.filename) {
            Some(id) => *id,
            None => {
                // create missing image
                let (w, h) = image_dims.get(&le.filename).copied().unwrap_or((1000.0, 1000.0));
                let id = project.add_image(le.filename.clone(), w, h);
                file_to_id.insert(le.filename.clone(), id);
                id
            }
        };
        let source = if le.is_manual.unwrap_or(false) { EntrySource::Manual } else { EntrySource::AutoOcr };
        let score = le.confidence.unwrap_or(0.9);
        let quad = Quad { points: le.coordinates };
        let new_entry = NewEntry { source, text: le.text.clone(), score, quad };
        let eid = project.ocr.append_for_image(image_id, new_entry);
        // handle deleted after append: need to set deleted flag
        if le.is_deleted.unwrap_or(false) {
            project.ocr.soft_delete(eid);
        }
        // custom_style -> EntryStyle : apply to project styles
        if let Some(style_val) = &le.custom_style
            && let Some(style) = legacy_style_to_entry_style(style_val) {
                project.set_entry_style(eid, style);
            }
        // translations per entry
        if let Some(trans) = &le.translations {
            for (pname, ttext) in trans {
                if pname == "Original" {
                    continue;
                }
                if let Some(pid) = project.profiles.find_by_name(pname) {
                    // we need to set translation for that profile without changing selected for other entries
                    // Use profiles.iter_mut find?
                    // To avoid messing with selected, we directly edit the profile's deltas via selecting temporarily? Better to directly access deltas via reconstruction
                    // We'll select, set, then restore
                    let prev = project.profiles.selected_id();
                    project.profiles.select(pid);
                    project.profiles.selected_mut().set_translation(eid, Some(ttext.clone()));
                    project.profiles.select(prev);
                }
            }
        }
    }

    // set active profile selection
    if let Some(active) = &meta.active_profile_name
        && let Some(pid) = project.profiles.find_by_name(active) {
            project.profiles.select(pid);
        }

    // fix next_id to be max row_number +1 if larger than current
    let max_row = master.iter().filter_map(|e| row_to_u64(&e.row_number)).max().unwrap_or(0);
    // project.ocr.next_id is currently len; if max_row+1 > next, we need to bump
    let desired = if max_row > 0 { max_row + 1 } else { project.ocr.next_id() };
    if desired > project.ocr.next_id() {
        // reconstruct OcrResult with desired next_id
        let entries = project.ocr.entries().to_vec();
        let ocr = OcrResult::from_raw(entries, desired);
        // reconstruct project with new ocr
        let images = project.images().to_vec();
        let next_image_id = project.next_image_id();
        let profiles = std::mem::take(&mut project.profiles);
        // but we have moved; instead build new project via from_raw
        let styles = project.styles().clone();
        let view_quads = project.view_quads().clone();
        let extras = std::mem::take(&mut project.extras);
        let new_proj = Project::from_raw(images, next_image_id, ocr, profiles, styles, view_quads, extras);
        return Ok(new_proj);
    }

    Ok(project)
}

fn row_to_f64(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}
fn row_to_u64(v: &serde_json::Value) -> Option<u64> {
    match v {
        serde_json::Value::Number(n) => n.as_u64().or_else(|| n.as_f64().map(|f| f as u64)),
        serde_json::Value::String(s) => s.parse::<f64>().ok().map(|f| f as u64),
        _ => None,
    }
}

fn legacy_style_to_entry_style(val: &serde_json::Value) -> Option<easyscanlate_model::EntryStyle> {
    // ManhwaOCR custom_style is diff vs DEFAULT_TEXT_STYLE.
    // We'll attempt to map known keys to our EntryStyle.
    // Keys seen: font_size, bg_color (#AARRGGBB), bubble_type, corner_radius, text_color, border_color, etc.
    // This is best-effort.
    let obj = val.as_object()?;
    let mut style = easyscanlate_model::EntryStyle::default();
    let mut changed = false;
    if let Some(fs) = obj.get("font_size").and_then(|v| v.as_f64()) {
        style.font_size = fs as f32;
        changed = true;
    }
    if let Some(bg) = obj.get("bg_color").and_then(|v| v.as_str())
        && let Some(rgba) = parse_hex_argb(bg) {
            style.bg_color = rgba;
            changed = true;
        }
    if let Some(tc) = obj.get("text_color").and_then(|v| v.as_str())
        && let Some(rgba) = parse_hex_argb(tc) {
            style.text_color = rgba;
            changed = true;
        }
    if let Some(bc) = obj.get("border_color").and_then(|v| v.as_str())
        && let Some(rgba) = parse_hex_argb(bc) {
            style.stroke_color = rgba;
            changed = true;
        }
    if let Some(bw) = obj.get("border_width").and_then(|v| v.as_f64()) {
        style.stroke_width = bw as f32;
        changed = true;
    }
    if let Some(cr) = obj.get("corner_radius").and_then(|v| v.as_f64()) {
        style.bg_radius = cr as f32;
        changed = true;
    }
    if let Some(fam) = obj.get("font_family").and_then(|v| v.as_str()) {
        style.font_family = Some(fam.to_string());
        changed = true;
    }
    if let Some(bold) = obj.get("font_bold").and_then(|v| v.as_bool()) {
        style.bold = bold;
        changed = true;
    }
    if let Some(italic) = obj.get("font_italic").and_then(|v| v.as_bool()) {
        style.italic = italic;
        changed = true;
    }
    if let Some(align) = obj.get("text_alignment").and_then(|v| v.as_u64()) {
        style.text_align = match align {
            0 => easyscanlate_model::TextAlign::Left,
            1 => easyscanlate_model::TextAlign::Center,
            2 => easyscanlate_model::TextAlign::Right,
            _ => easyscanlate_model::TextAlign::Circular,
        };
        changed = true;
    }
    if changed { Some(style) } else { None }
}

fn parse_hex_argb(s: &str) -> Option<[u8; 4]> {
    // #AARRGGBB or #RRGGBB or #ARGB?
    let hex = s.trim_start_matches('#');
    if hex.len() == 8 {
        let a = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let r = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let g = u8::from_str_radix(&hex[4..6], 16).ok()?;
        let b = u8::from_str_radix(&hex[6..8], 16).ok()?;
        Some([r, g, b, a])
    } else if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some([r, g, b, 255])
    } else {
        None
    }
}

fn entry_style_to_legacy_custom(val: &easyscanlate_model::EntryStyle) -> Option<serde_json::Value> {
    let def = easyscanlate_model::EntryStyle::default();
    if val == &def {
        return None;
    }
    let mut map = serde_json::Map::new();
    if val.font_size != def.font_size {
        map.insert("font_size".into(), serde_json::Value::Number(serde_json::Number::from_f64(val.font_size as f64).unwrap()));
    }
    if val.bg_color != def.bg_color {
        map.insert("bg_color".into(), serde_json::Value::String(rgba_to_hex(val.bg_color)));
    }
    if val.text_color != def.text_color {
        map.insert("text_color".into(), serde_json::Value::String(rgba_to_hex(val.text_color)));
    }
    if val.stroke_color != def.stroke_color {
        map.insert("border_color".into(), serde_json::Value::String(rgba_to_hex(val.stroke_color)));
    }
    if val.stroke_width != def.stroke_width {
        map.insert("border_width".into(), serde_json::Value::Number(serde_json::Number::from_f64(val.stroke_width as f64).unwrap()));
    }
    if val.bg_radius != def.bg_radius {
        map.insert("corner_radius".into(), serde_json::Value::Number(serde_json::Number::from_f64(val.bg_radius as f64).unwrap()));
    }
    if val.font_family != def.font_family
        && let Some(fam) = &val.font_family {
            map.insert("font_family".into(), serde_json::Value::String(fam.clone()));
        }
    if val.bold != def.bold {
        map.insert("font_bold".into(), serde_json::Value::Bool(val.bold));
    }
    if val.italic != def.italic {
        map.insert("font_italic".into(), serde_json::Value::Bool(val.italic));
    }
    if val.text_align != def.text_align {
        let v = match val.text_align {
            easyscanlate_model::TextAlign::Left => 0,
            easyscanlate_model::TextAlign::Center => 1,
            easyscanlate_model::TextAlign::Right => 2,
            easyscanlate_model::TextAlign::Circular => 1,
        };
        map.insert("text_alignment".into(), serde_json::Value::Number(serde_json::Number::from(v)));
    }
    if map.is_empty() { None } else { Some(serde_json::Value::Object(map)) }
}

fn rgba_to_hex(c: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}{:02x}", c[3], c[0], c[1], c[2])
}

// Convert Project to legacy master entries for export
pub fn project_to_legacy_master(project: &Project) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    // need mapping image_id -> filename
    let id_to_file: HashMap<ImageId, String> = project.images().iter().map(|m| (m.id, m.path.clone())).collect();
    // for filename, use basename only (like legacy)
    for entry in project.ocr.entries() {
        // use entry.id.0+1 as row_number to keep uniqueness, but mimic legacy row_number sequential
        let row_number = entry.id.0 + 1;
        let filename = id_to_file.get(&entry.image_id).map(|p| {
            std::path::Path::new(p).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| p.clone())
        }).unwrap_or_else(|| "unknown.jpg".to_string());
        let mut obj = serde_json::Map::new();
        obj.insert("row_number".into(), serde_json::Value::Number(serde_json::Number::from(row_number)));
        obj.insert("filename".into(), serde_json::Value::String(filename));
        obj.insert("coordinates".into(), serde_json::json!(entry.quad.points));
        obj.insert("text".into(), serde_json::Value::String(entry.text.clone()));
        obj.insert("confidence".into(), serde_json::Value::Number(serde_json::Number::from_f64(entry.score as f64).unwrap_or(serde_json::Number::from(0))));
        // translations
        let mut trans = serde_json::Map::new();
        for prof in project.profiles.iter() {
            if let Some(t) = prof.translation_of(entry.id) {
                trans.insert(prof.name.clone(), serde_json::Value::String(t.to_string()));
            }
        }
        if !trans.is_empty() {
            obj.insert("translations".into(), serde_json::Value::Object(trans));
        }
        if let Some(custom) = entry_style_to_legacy_custom(&project.entry_style(entry.id)) {
            obj.insert("custom_style".into(), custom);
        }
        if entry.deleted {
            obj.insert("is_deleted".into(), serde_json::Value::Bool(true));
        }
        if entry.source == easyscanlate_model::EntrySource::Manual {
            obj.insert("is_manual".into(), serde_json::Value::Bool(true));
        }
        out.push(serde_json::Value::Object(obj));
    }
    out
}

pub fn project_to_legacy_meta(project: &Project) -> serde_json::Value {
    serde_json::json!({
        "original_language": "Korean",
        "active_profile_name": project.profiles.selected().name,
        "version": "1.0"
    })
}

// Helpers for Zip import/export used by zip.rs
pub fn parse_legacy_meta_slice(data: &[u8]) -> Result<LegacyMeta, String> {
    serde_json::from_slice::<LegacyMeta>(data).map_err(|e| e.to_string())
}
pub fn parse_legacy_master_slice(data: &[u8]) -> Result<Vec<LegacyEntry>, String> {
    serde_json::from_slice::<Vec<LegacyEntry>>(data).map_err(|e| e.to_string())
}
pub fn parse_legacy_inpaint_slice(data: &[u8]) -> Result<Vec<LegacyInpaintRecord>, String> {
    serde_json::from_slice::<Vec<LegacyInpaintRecord>>(data).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn style_hex_roundtrip() {
        let c = [10, 20, 30, 40];
        let h = rgba_to_hex(c);
        assert_eq!(parse_hex_argb(&h).unwrap(), c);
    }
}
