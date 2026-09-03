//! ZIP wrapper for .mmtl : project.xml + images/ + inpaint/
//! `project.xml` stores `ImageMeta.path` zip-relative (`images/<id>_<basename>`);
//! on load it is resolved to an absolute temp path (`LoadResult.temp_dir`).

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use easyscanlate_model::{ImageId, Project};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

use crate::legacy;
use crate::xml::{from_xml_str, to_xml_string};

#[derive(Debug, Clone)]
pub struct InpaintImageData {
    pub image_id: ImageId,
    pub bounds: [f32; 4],
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>, // width*height*4
}

#[derive(Debug)]
pub struct LoadResult {
    pub project: Project,
    /// Absolute paths of extracted images (temp dir). Caller should keep temp dir alive.
    pub image_paths: HashMap<ImageId, PathBuf>,
    /// Temp dir that holds extracted files (caller must keep alive or copy).
    pub temp_dir: tempfile::TempDir,
    /// Patch PNGs extracted (image_id, bounds, png path)
    pub inpaint_files: Vec<(ImageId, [f32; 4], PathBuf)>,
}

pub fn save_mmtl(
    project: &Project,
    inpaint_images: &[InpaintImageData],
    dest: &Path,
) -> Result<(), String> {
    // Validate dest ends with .mmtl
    let dest = if dest.extension().map(|e| e.to_string_lossy().to_lowercase()) != Some("mmtl".to_string()) {
        let mut p = dest.as_os_str().to_owned();
        p.push(".mmtl");
        PathBuf::from(p)
    } else {
        dest.to_path_buf()
    };

    // Build zip-relative paths: images/<id>_<basename> (always, keeps unique)
    let mut id_to_rel: HashMap<ImageId, String> = HashMap::new();
    for meta in project.images() {
        let file_name = Path::new(&meta.path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("{}.png", meta.id.0));
        let rel = format!("images/{}_{}", meta.id.0, file_name);
        id_to_rel.insert(meta.id, rel);
    }
    // Serialize project.xml with relative paths (inside .mmtl the path is zip-relative)
    let rel_images: Vec<ImageMeta> = project
        .images()
        .iter()
        .map(|m| ImageMeta {
            id: m.id,
            path: id_to_rel.get(&m.id).cloned().unwrap_or_else(|| m.path.clone()),
            width: m.width,
            height: m.height,
        })
        .collect();
    let rel_project = Project::from_raw(
        rel_images,
        project.next_image_id(),
        project.ocr.clone(),
        project.profiles.clone(),
        project.styles().clone(),
        project.view_quads().clone(),
        project.extras.clone(),
    );
    let xml = to_xml_string(&rel_project)?;
    // create zip
    let file = File::create(&dest).map_err(|e| format!("create {dest:?}: {e}"))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // project.xml
    zip.start_file("project.xml", options).map_err(|e| e.to_string())?;
    zip.write_all(xml.as_bytes()).map_err(|e| e.to_string())?;

    // images/ (read from original absolute/temp path, write to relative zip path)
    for meta in project.images() {
        let src_path = &meta.path;
        let rel = id_to_rel
            .get(&meta.id)
            .cloned()
            .unwrap_or_else(|| format!("images/{}.png", meta.id.0));
        let data = std::fs::read(src_path).map_err(|e| format!("read image {src_path}: {e}"))?;
        zip.start_file(rel, options).map_err(|e| e.to_string())?;
        zip.write_all(&data).map_err(|e| e.to_string())?;
    }

    // inpaint/ — per-image sequential naming (0..n for each image) so
    // load can map by image_id + per-image ordinal regardless of global order.
    // Previous versions used global enumerate idx (bug: load treated it as per-image).
    let mut per_image_next: HashMap<ImageId, usize> = HashMap::new();
    for patch in inpaint_images.iter() {
        let cnt = per_image_next.entry(patch.image_id).or_insert(0);
        let per_idx = *cnt;
        *cnt += 1;
        let name = format!("inpaint/{}_{}.png", patch.image_id.0, per_idx);
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        // encode rgba to PNG
        let mut buf = Vec::new();
        {
            use image::ImageEncoder;
            let enc = image::codecs::png::PngEncoder::new(&mut buf);
            let rgba = image::RgbaImage::from_raw(patch.width, patch.height, patch.rgba.clone())
                .ok_or_else(|| "invalid rgba size".to_string())?;
            enc.write_image(
                rgba.as_raw(),
                patch.width,
                patch.height,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
        }
        zip.write_all(&buf).map_err(|e| e.to_string())?;
    }

    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn save_legacy_mmtl(project: &Project, dest: &Path) -> Result<(), String> {
    let dest = if dest.extension().map(|e| e.to_string_lossy().to_lowercase()) != Some("mmtl".to_string()) {
        let mut p = dest.as_os_str().to_owned();
        p.push(".mmtl");
        PathBuf::from(p)
    } else {
        dest.to_path_buf()
    };
    let master = legacy::project_to_legacy_master(project);
    let meta = legacy::project_to_legacy_meta(project);
    let master_json = serde_json::to_string_pretty(&master).map_err(|e| e.to_string())?;
    let meta_json = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;

    let file = File::create(&dest).map_err(|e| format!("create {dest:?}: {e}"))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("master.json", options).map_err(|e| e.to_string())?;
    zip.write_all(master_json.as_bytes()).map_err(|e| e.to_string())?;
    zip.start_file("meta.json", options).map_err(|e| e.to_string())?;
    zip.write_all(meta_json.as_bytes()).map_err(|e| e.to_string())?;
    // images
    for meta_img in project.images() {
        let src_path = &meta_img.path;
        let file_name = Path::new(src_path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("{}.png", meta_img.id.0));
        let zip_path = format!("images/{}_{}", meta_img.id.0, file_name);
        if let Ok(data) = std::fs::read(src_path) {
            zip.start_file(zip_path, options).map_err(|e| e.to_string())?;
            zip.write_all(&data).map_err(|e| e.to_string())?;
        }
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_mmtl(path: &Path) -> Result<LoadResult, String> {
    let file = File::open(path).map_err(|e| format!("open {path:?}: {e}"))?;
    let mut archive = ZipArchive::new(file).map_err(|e| format!("zip open: {e}"))?;
    // check if project.xml exists
    let mut has_xml = false;
    let mut has_master = false;
    for i in 0..archive.len() {
        if let Ok(f) = archive.by_index(i) {
            let name = f.name().to_string();
            if name == "project.xml" { has_xml = true; }
            if name == "master.json" { has_master = true; }
        }
    }
    if has_xml {
        load_native_zip(archive)
    } else if has_master {
        load_legacy_zip(archive)
    } else {
        Err("Invalid .mmtl: missing project.xml and master.json".to_string())
    }
}

fn load_native_zip(mut archive: ZipArchive<File>) -> Result<LoadResult, String> {
    // extract to temp dir
    let temp = tempfile::TempDir::new().map_err(|e| e.to_string())?;
    let mut xml_data: Option<String> = None;
    let mut inpaint_names: Vec<String> = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().to_string();
        let out_path = temp.path().join(&name);
        if file.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = File::create(&out_path).map_err(|e| e.to_string())?;
        std::io::copy(&mut file, &mut out).map_err(|e| e.to_string())?;
        if name == "project.xml" {
            let mut buf = String::new();
            File::open(&out_path).map_err(|e| e.to_string())?.read_to_string(&mut buf).map_err(|e| e.to_string())?;
            xml_data = Some(buf);
        } else if name.starts_with("inpaint/") {
            inpaint_names.push(name);
        }
    }
    let xml = xml_data.ok_or_else(|| "missing project.xml after extract".to_string())?;
    let mut project = from_xml_str(&xml)?;
    // project.xml stores zip-relative paths (images/<id>_<basename>).
    // Resolve each to the extracted temp absolute path.
    let mut id_to_path: HashMap<ImageId, PathBuf> = HashMap::new();
    for meta in project.images().to_vec() {
        let rel = Path::new(&meta.path);
        let found = temp.path().join(rel);
        if !found.exists() {
            return Err(format!(
                "missing image file for id {}: expected {} in .mmtl",
                meta.id.0,
                meta.path
            ));
        }
        id_to_path.insert(meta.id, found);
    }
    // Rebuild project with updated paths
    let mut new_images = Vec::new();
    for meta in project.images().iter() {
        let new_path = id_to_path.get(&meta.id).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| meta.path.clone());
        // optionally re-read dimensions from extracted file
        let (w, h) = if let Some(p) = id_to_path.get(&meta.id) {
            if let Ok(img) = image::ImageReader::open(p).and_then(|r| r.with_guessed_format().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))) {
                if let Ok(decoded) = img.decode() {
                    (decoded.width() as f32, decoded.height() as f32)
                } else { (meta.width, meta.height) }
            } else { (meta.width, meta.height) }
        } else { (meta.width, meta.height) };
        new_images.push(ImageMeta { id: meta.id, path: new_path, width: w, height: h });
    }
    if !new_images.is_empty() {
        // reconstruct project with new images paths
        let ocr = std::mem::replace(&mut project.ocr, OcrResult::from_raw(Vec::new(), 0));
        let profiles = std::mem::replace(&mut project.profiles, easyscanlate_model::Profiles::default());
        let styles = std::mem::replace(&mut project.styles().clone(), HashMap::new());
        // need mutable access to private fields via from_raw
        // Instead use Project::from_raw
        // we need to extract all fields
        let extras = std::mem::replace(&mut project.extras, easyscanlate_model::Extras::default());
        let view_quads = project.view_quads().clone();
        let styles_map = styles;
        // Note: project was moved partially, but we have its components saved
        // For simplicity, reconstruct fresh project and then copy ocr dims if changed
        // We already consumed ocr, profiles etc.
        // Let's build new project correctly by using from_raw with collected data
        // We lost next_image_id, so recompute
        let next_image_id = new_images.iter().map(|m| m.id.0+1).max().unwrap_or(0);
        let rebuilt = Project::from_raw(new_images, next_image_id, ocr, profiles, styles_map, view_quads, extras);
        project = rebuilt;
    }

    // inpaint files — robust mapping that handles both new per-image naming
    // and legacy global-idx naming. New saves use per-image ordinal (0..n).
    // Old saves used global enumerate idx, which exceeds per-image len for
    // most images and caused 0×0 fallback (bug). We therefore group files
    // by image_id and assign sequentially by patch order, with dimension
    // verification to catch mismatches.
    let mut inpaint_files = Vec::new();
    // Parse each name into (image_id, raw_idx, PathBuf, full_name)
    struct Parsed { image_id: ImageId, raw_idx: Option<usize>, path: PathBuf, name: String }
    let mut parsed: Vec<Parsed> = Vec::new();
    for name in &inpaint_names {
        let path = temp.path().join(name);
        let stem = Path::new(name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let mut parts = stem.split('_');
        if let (Some(id_str), Some(idx_str)) = (parts.next(), parts.next()) {
            if let Ok(id) = id_str.parse::<u64>() {
                let raw_idx = idx_str.parse::<usize>().ok();
                parsed.push(Parsed { image_id: ImageId(id), raw_idx, path: path.clone(), name: name.clone() });
                continue;
            }
        }
        // unparsable -> push with dummy id 0 (will be skipped)
        parsed.push(Parsed { image_id: ImageId(0), raw_idx: None, path: path.clone(), name: name.clone() });
    }
    // Group by image_id, sort each group by raw_idx (or name) for stable order.
    let mut groups: HashMap<ImageId, Vec<Parsed>> = HashMap::new();
    for p in parsed {
        // skip dummy unparsable that looked like inpaint/.png with no id
        if p.image_id == ImageId(0) && p.raw_idx.is_none() {
            // try to still expose as fallback entry so user sees something
            inpaint_files.push((ImageId(0), [0.0;4], p.path));
            continue;
        }
        groups.entry(p.image_id).or_default().push(p);
    }
    for (image_id, mut entries) in groups {
        // sort by raw_idx numeric if present, else lexicographically; this
        // makes legacy global-idx files (e.g. 19_5.png,19_6.png,19_7.png) fall
        // into per-image sequential order 5->0,6->1,7->2 after grouping.
        entries.sort_by(|a, b| {
            match (a.raw_idx, b.raw_idx) {
                (Some(ai), Some(bi)) => ai.cmp(&bi),
                _ => a.name.cmp(&b.name),
            }
        });
        // Patches for this image in insertion order (the canonical store)
        let patches_for_image: Vec<_> = project.extras.inpaint_patches.iter().filter(|p| p.image_id==image_id).collect();
        // If file count != patch count, still assign 1:1 in sorted order up to min;
        // extras may be ground truth length, so use min.
        // We attempt dimension-aware matching to detect ordering drift:
        // Build map of not-yet-assigned patch indices.
        // First pass: try to match by dimensions (PNG w/h == patch w/h) for robustness.
        let mut assigned: Vec<bool> = vec![false; patches_for_image.len()];
        // Pre-read dimensions for entries (cheap: read header via image crate)
        let mut entry_dims: Vec<Option<(u32,u32)>> = Vec::new();
        for e in &entries {
            let dim = image::ImageReader::open(&e.path)
                .and_then(|r| r.with_guessed_format().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))
                .and_then(|r| r.into_dimensions().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))
                .ok();
            // into_dimensions returns (w,h); but we called via ImageReader? Actually with_guessed_format returns Reader; use .into_dimensions
            // If we used ImageReader::open + with_guessed_format, we get an ImageReader; into_dimensions works without decoding full.
            entry_dims.push(dim);
        }
        // Try dimension match
        let mut matched_by_dim: Vec<Option<usize>> = vec![None; entries.len()];
        for (ei, dim) in entry_dims.iter().enumerate() {
            if let Some((w,h)) = dim {
                let mut best: Option<usize> = None;
                for (pi, patch) in patches_for_image.iter().enumerate() {
                    if assigned[pi] { continue; }
                    if patch.bounds[2] as u32 == *w && patch.bounds[3] as u32 == *h {
                        best = Some(pi);
                        break;
                    }
                }
                if let Some(pi) = best {
                    assigned[pi] = true;
                    matched_by_dim[ei] = Some(pi);
                }
            }
        }
        // Now emit inpaint_files: for each entry in sorted order, use dimension match if found,
        // else fallback to sequential next unassigned per-image ordinal.
        let mut next_seq = 0usize;
        for (ei, entry) in entries.iter().enumerate() {
            if let Some(pi) = matched_by_dim[ei] {
                let patch = patches_for_image[pi];
                inpaint_files.push((patch.image_id, patch.bounds, entry.path.clone()));
            } else {
                // find next unassigned sequential
                while next_seq < patches_for_image.len() && assigned[next_seq] {
                    next_seq += 1;
                }
                if next_seq < patches_for_image.len() {
                    let patch = patches_for_image[next_seq];
                    assigned[next_seq] = true;
                    inpaint_files.push((patch.image_id, patch.bounds, entry.path.clone()));
                    next_seq += 1;
                } else {
                    // More files than patches (or patches missing) -> fallback zero
                    // Still expose file with dummy bounds so UI shows something instead of silently dropping.
                    // Try raw_idx direct if possible
                    if let Some(raw) = entry.raw_idx {
                        if let Some(patch) = patches_for_image.get(raw) {
                            inpaint_files.push((patch.image_id, patch.bounds, entry.path.clone()));
                            continue;
                        }
                    }
                    inpaint_files.push((entry.image_id, [0.0;4], entry.path.clone()));
                }
            }
        }
    }

    // Build map image_id -> path
    let mut image_paths: HashMap<ImageId, PathBuf> = HashMap::new();
    for (id, path) in &id_to_path {
        image_paths.insert(*id, path.clone());
    }
    Ok(LoadResult { project, image_paths, temp_dir: temp, inpaint_files })
}

fn load_legacy_zip(mut archive: ZipArchive<File>) -> Result<LoadResult, String> {
    let temp = tempfile::TempDir::new().map_err(|e| e.to_string())?;
    let mut master_data: Option<Vec<u8>> = None;
    let mut meta_data: Option<Vec<u8>> = None;
    let mut image_names: Vec<String> = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().to_string();
        let out_path = temp.path().join(&name);
        if file.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // buffer content
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        // also write to temp for images
        if name.starts_with("images/") {
            std::fs::write(&out_path, &buf).map_err(|e| e.to_string())?;
            image_names.push(name);
        } else if name == "master.json" {
            master_data = Some(buf.clone());
            std::fs::write(&out_path, &buf).map_err(|e| e.to_string())?;
        } else if name == "meta.json" {
            meta_data = Some(buf.clone());
            std::fs::write(&out_path, &buf).map_err(|e| e.to_string())?;
        } else if name.starts_with("inpaint/") {
            std::fs::write(&out_path, &buf).map_err(|e| e.to_string())?;
        }
    }
    let master_bytes = master_data.ok_or_else(|| "missing master.json".to_string())?;
    let meta_bytes = meta_data.ok_or_else(|| "missing meta.json".to_string())?;
    let legacy_master = legacy::parse_legacy_master_slice(&master_bytes)?;
    let legacy_meta = legacy::parse_legacy_meta_slice(&meta_bytes)?;
    // Build image dims map from extracted files
    let mut dims: HashMap<String, (f32,f32)> = HashMap::new();
    for name in &image_names {
        let path = temp.path().join(name);
        let filename = Path::new(name).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if let Ok(reader) = image::ImageReader::open(&path) {
            if let Ok(reader) = reader.with_guessed_format() {
                if let Ok(img) = reader.decode() {
                    dims.insert(filename, (img.width() as f32, img.height() as f32));
                }
            }
        }
    }
    let mut project = legacy::project_from_legacy(&legacy_master, &legacy_meta, &dims)?;
    // Update paths to extracted absolute paths
    let mut new_images: Vec<ImageMeta> = Vec::new();
    for meta in project.images().iter() {
        let basename = Path::new(&meta.path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| meta.path.clone());
        // find extracted file path that matches basename
        let mut found_path = None;
        for n in &image_names {
            if n.ends_with(&basename) {
                found_path = Some(temp.path().join(n));
                break;
            }
        }
        let abs_path = found_path.map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| meta.path.clone());
        new_images.push(ImageMeta { id: meta.id, path: abs_path, width: meta.width, height: meta.height });
    }
    // reconstruct with new paths
    let ocr = std::mem::replace(&mut project.ocr, OcrResult::from_raw(Vec::new(), 0));
    let profiles = std::mem::replace(&mut project.profiles, easyscanlate_model::Profiles::default());
    let styles = project.styles().clone();
    let view_quads = project.view_quads().clone();
    let extras = std::mem::replace(&mut project.extras, easyscanlate_model::Extras::default());
    let next_image_id = new_images.iter().map(|m| m.id.0+1).max().unwrap_or(0);
    let rebuilt = Project::from_raw(new_images.clone(), next_image_id, ocr, profiles, styles, view_quads, extras);
    let mut image_paths = HashMap::new();
    for m in &new_images {
        image_paths.insert(m.id, PathBuf::from(&m.path));
    }
    Ok(LoadResult { project: rebuilt, image_paths, temp_dir: temp, inpaint_files: Vec::new() })
}

use easyscanlate_model::{OcrResult, ImageMeta};

#[cfg(test)]
mod tests {
    use super::*;
    use easyscanlate_model::{EntrySource, NewEntry, Quad, Project, ImageId};

    #[test]
    fn save_and_load_roundtrip() {
        let mut project = Project::new();
        let id = project.add_image("test.png", 100.0, 100.0);
        // create a dummy image file
        let tmp = tempfile::TempDir::new().unwrap();
        let img_path = tmp.path().join("test.png");
        let img = image::RgbaImage::new(10, 10);
        img.save(&img_path).unwrap();
        // update project path to real file
        let mut images = project.images().to_vec();
        images[0].path = img_path.to_string_lossy().to_string();
        let ocr = std::mem::replace(&mut project.ocr, OcrResult::from_raw(Vec::new(),0));
        let profiles = std::mem::replace(&mut project.profiles, easyscanlate_model::Profiles::default());
        let styles = project.styles().clone();
        let view_quads = project.view_quads().clone();
        let extras = std::mem::replace(&mut project.extras, easyscanlate_model::Extras::default());
        let project2 = Project::from_raw(images, 1, ocr, profiles, styles, view_quads, extras);
        let mut project = project2;
        project.ocr.append_for_image(id, NewEntry{ source: EntrySource::AutoOcr, text:"hi".into(), score:0.9, quad: Quad{points:[[0.0,0.0],[10.0,0.0],[10.0,10.0],[0.0,10.0]]}});
        let dest = tmp.path().join("out.mmtl");
        save_mmtl(&project, &[], &dest).unwrap();
        let loaded = load_mmtl(&dest).unwrap();
        assert_eq!(loaded.project.images().len(), 1);
        assert_eq!(loaded.project.ocr.entries().len(), 1);
    }
}
