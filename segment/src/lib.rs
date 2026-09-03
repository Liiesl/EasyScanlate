//! Segmentation (panel / balloon / SFX) inference for manga-mimic grids.
//! DirectML execution provider by default on Windows (feature `directml`), with CPU fallback.
//!
//! Wraps `yolo26s-seg.onnx` @1024 square (classes: frame, dialogue_text, balloon,
//! onomatopoeia_text). After OCR finishes the app builds ratio-based grid
//! canvases via [`grid::plan_grids`], runs this engine on each canvas, maps
//! detections back to native page coords, then filters SFX OCR hallucinations
//! via [`filter`].

use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use image::RgbImage;
use ndarray::{Array4, ArrayD};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::TensorRef;

pub mod filter;
pub mod grid;

/// Model inference size.
pub const IMG_SIZE: u32 = grid::IMG_SIZE;

/// Class names in the Koharu ONNX.
pub const CLASSES: &[&str] = &["frame", "dialogue_text", "balloon", "onomatopoeia_text"];

const MODEL_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../models");
const MODEL_FILE_KOHARU: &str = "yolo26s-seg.onnx";
const MODEL_FILE_FALLBACK: &str = "best.onnx";

/// One segmentation detection in canvas space (square `side x side` after grid
/// building, before mapping to page).
#[derive(Debug, Clone)]
pub struct SegDet {
    pub class: SegClass,
    pub class_id: usize,
    pub confidence: f32,
    /// `[x1,y1,x2,y2]` in canvas square pixels (side x side, pre-letterbox invert if letterboxed).
    pub bbox: [f32; 4],
    /// Binary mask of the instance, in canvas sub-region (optional, for future mask-precise filtering).
    pub mask: Option<Mask>, // not yet used by filter (box-only)
    /// Origin of the mask in canvas space.
    pub mask_origin: Option<(i32, i32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegClass {
    Frame,
    DialogueText,
    Balloon,
    Onomatopoeia,
    Unknown,
}

impl SegClass {
    pub fn from_id(id: usize) -> Self {
        match id {
            0 => SegClass::Frame,
            1 => SegClass::DialogueText,
            2 => SegClass::Balloon,
            3 => SegClass::Onomatopoeia,
            _ => SegClass::Unknown,
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            SegClass::Frame => "frame",
            SegClass::DialogueText => "dialogue_text",
            SegClass::Balloon => "balloon",
            SegClass::Onomatopoeia => "onomatopoeia_text",
            SegClass::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Mask {
    pub data: Vec<u8>, // row-major 0/1
    pub width: u32,
    pub height: u32,
}

/// Cloneable handle to the shared segmentation session.
#[derive(Clone)]
pub struct Engine(Arc<Mutex<Session>>);

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SegmentEngine")
    }
}

impl Engine {
    pub fn build() -> Result<Self, String> {
        // Prefer onboarding-downloaded models (koharu-yolo26s-seg.onnx) then legacy names.
        // Downloaded canonical filename is `koharu-yolo26s-seg.onnx` (registry MODELS).
        // Keep legacy fallbacks for dev `yolo26s-seg.onnx`, `best.onnx`, and alt folder.
        let canonical = easyscanlate_settings::resolve_model_path_with_legacy(
            "koharu-yolo26s-seg.onnx",
            Some(MODEL_FILE_KOHARU),
        );
        let fallback = easyscanlate_settings::resolve_model_path(MODEL_FILE_FALLBACK);
        let alt = Path::new(MODEL_DIR)
            .join("../onnx-text-styling-classification/panel-bubble-sfx-det")
            .join(MODEL_FILE_KOHARU);
        let legacy_koharu = Path::new(MODEL_DIR).join(MODEL_FILE_KOHARU);
        // Priority: canonical (including settings models_dir), then legacy names, then alt.
        let path = if canonical.exists() {
            canonical
        } else if legacy_koharu.exists() {
            legacy_koharu
        } else if fallback.exists() {
            fallback
        } else if alt.exists() {
            alt
        } else {
            return Err(format!(
                "segmentation model not found: tried {}, {} and {} (and settings models_dir)",
                canonical.display(),
                fallback.display(),
                alt.display()
            ));
        };
        // Helper: CPU session (fallback). Keep ARNs calm: like OCR we disable memory pattern (dynamic 1024) and CPU arena —
        // otherwise each 1024 canvas triggers mmap arena grow/shrink sawtooth (+500 residual).
        let build_cpu = || -> Result<Session, String> {
            Session::builder()
                .map_err(|e| format!("ORT init failed: {e}"))?
                .with_optimization_level(GraphOptimizationLevel::Level3)
                .map_err(|e| format!("ORT init failed: {e}"))?
                .with_intra_threads(2)
                .map_err(|e| format!("ORT init failed: {e}"))?
                .with_memory_pattern(false)
                .map_err(|e| format!("ORT init failed: {e}"))?
                .with_execution_providers([ort::ep::CPU::default()
                    .with_arena_allocator(false)
                    .build()])
                .map_err(|e| format!("ORT init failed: {e}"))?
                .commit_from_file(&path)
                .map_err(|e| format!("failed to load segmentation model {}: {e}", path.display()))
        };

        #[cfg(all(feature = "directml", target_os = "windows"))]
        let build_directml = || -> Result<Session, String> {
            Session::builder()
                .map_err(|e| format!("ORT init failed: {e}"))?
                .with_optimization_level(GraphOptimizationLevel::Level3)
                .map_err(|e| format!("ORT init failed: {e}"))?
                .with_intra_threads(2)
                .map_err(|e| format!("ORT init failed: {e}"))?
                .with_memory_pattern(false)
                .map_err(|e| format!("ORT init failed: {e}"))?
                .with_execution_providers([ort::ep::DirectML::default()
                    .build()
                    .error_on_failure()])
                .map_err(|e| format!("ORT init failed: {e}"))?
                .commit_from_file(&path)
                .map_err(|e| format!("failed to load segmentation model {}: {e}", path.display()))
        };

        #[cfg(all(feature = "directml", target_os = "windows"))]
        let session = match build_directml() {
            Ok(s) => {
                eprintln!("[segment] DirectML EP active for {}", path.display());
                s
            }
            Err(e) => {
                eprintln!(
                    "[segment] DirectML EP init failed for {}: {e} – falling back to CPU",
                    path.display()
                );
                build_cpu()?
            }
        };
        #[cfg(any(not(feature = "directml"), not(target_os = "windows")))]
        let session = build_cpu()?;
        Ok(Self(Arc::new(Mutex::new(session))))
    }

    /// Run detection on a square grid canvas (`side x side` RGB). Returns
    /// detections in canvas square coords (after letterbox invert).
    pub fn detect_canvas(&self, canvas: &RgbImage) -> Result<Vec<SegDet>, String> {
        let mut session = self
            .0
            .lock()
            .map_err(|e| format!("Segment engine lock poisoned: {e}"))?;
        let (lb, r, dx, dy) = letterbox(canvas, IMG_SIZE);
        let input = compose_input(&lb);
        let outputs = run_session(&mut *session, input)?;
        let dets = decode_koharu(&outputs, r, dx, dy, canvas.width(), canvas.height());
        Ok(dets)
    }
}

fn letterbox(img: &RgbImage, size: u32) -> (RgbImage, f32, i32, i32) {
    let (w, h) = (img.width(), img.height());
    let r = (size as f32 / w as f32).min(size as f32 / h as f32);
    let nw = (w as f32 * r).round() as u32;
    let nh = (h as f32 * r).round() as u32;
    let resized = image::imageops::resize(img, nw, nh, image::imageops::FilterType::Triangle);
    let mut canvas = RgbImage::from_pixel(size, size, image::Rgb([114, 114, 114]));
    let dx = ((size - nw) / 2) as i32;
    let dy = ((size - nh) / 2) as i32;
    for y in 0..nh {
        for x in 0..nw {
            canvas.put_pixel((dx + x as i32) as u32, (dy + y as i32) as u32, *resized.get_pixel(x, y));
        }
    }
    (canvas, r, dx, dy)
}

fn compose_input(img: &RgbImage) -> Array4<f32> {
    debug_assert_eq!(img.dimensions(), (IMG_SIZE, IMG_SIZE));
    Array4::from_shape_fn((1, 3, IMG_SIZE as usize, IMG_SIZE as usize), |(_, c, y, x)| {
        let px = img.get_pixel(x as u32, y as u32);
        px[c] as f32 / 255.0
    })
}

fn run_session(session: &mut Session, input: Array4<f32>) -> Result<Vec<ArrayD<f32>>, String> {
    let outputs = session
        .run(ort::inputs![TensorRef::from_array_view(&input).map_err(|e| format!("{e}"))?])
        .map_err(|e| format!("Segment inference failed: {e}"))?;
    outputs
        .iter()
        .map(|(_, v)| {
            v.try_extract_array::<f32>()
                .map_err(|e| format!("output extract failed: {e}"))
                .map(|a| a.to_owned())
        })
        .collect()
}

fn sigmoid(x: f32) -> f32 {
    if x >= 0.0 {
        1.0 / (1.0 + (-x).exp())
    } else {
        x.exp() / (1.0 + x.exp())
    }
}


/// Decode Koharu outputs. Pure translation of `grid_pages.py:50-102`.
fn decode_koharu(
    outputs: &[ArrayD<f32>],
    r: f32,
    dx: i32,
    dy: i32,
    _orig_w: u32,
    _orig_h: u32,
) -> Vec<SegDet> {
    if outputs.len() < 2 {
        return Vec::new();
    }
    // outputs[0] is [1,300,38] or [300,38], outputs[1] is [1,32,320,320] or [32,320,320]
    // Normalize to 2D/3D.
    let out0 = &outputs[0];
    let out1 = &outputs[1];
    // Try to get shape info.
    let det_shape = out0.shape();
    let proto_shape = out1.shape();
    // Extract det as Vec<Vec<f32>> conceptually; handle both [300,38] and [1,300,38]
    let (num_det, det_dim) = if det_shape.len() == 3 {
        (det_shape[1], det_shape[2])
    } else if det_shape.len() == 2 {
        (det_shape[0], det_shape[1])
    } else {
        return Vec::new();
    };
    if det_dim < 6 {
        return Vec::new();
    }
    // proto: get [32,320,320] regardless of batch
    let (ph, pw) = if proto_shape.len() == 4 {
        (proto_shape[2] as i32, proto_shape[3] as i32)
    } else if proto_shape.len() == 3 {
        (proto_shape[1] as i32, proto_shape[2] as i32)
    } else {
        (320, 320)
    };
    let grid_h = ph as f32;
    let grid_w = pw as f32;
    // Flatten views
    let det_data: Vec<f32> = out0.iter().copied().collect();
    let proto_data: Vec<f32> = out1.iter().copied().collect();
    // Determine proto channel start offset if batched
    let proto_chan_stride = (ph * pw) as usize;
    // proto is [32,320,320] or [1,32,320,320] -> need to handle.
    // If shape len 4, first dim is batch, so channel 0 starts at 0.
    // If len 3, same.
    let mut out = Vec::new();
    let conf_thres = 0.25f32;
    // Collect candidates above threshold first for sorting/NMS.
    struct RawDet {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        conf: f32,
        cls: usize,
    }
    let mut raws: Vec<RawDet> = Vec::with_capacity(num_det);
    for i in 0..num_det {
        let base = i * det_dim;
        if base + 6 > det_data.len() {
            break;
        }
        let conf = det_data[base + 4];
        if conf <= conf_thres {
            continue;
        }
        let cls = det_data[base + 5] as usize;
        if cls >= CLASSES.len() {
            continue;
        }
        raws.push(RawDet {
            x1: det_data[base],
            y1: det_data[base + 1],
            x2: det_data[base + 2],
            y2: det_data[base + 3],
            conf,
            cls,
        });
    }
    // Sort by confidence descending, keep topK to cap work.
    raws.sort_by(|a, b| b.conf.partial_cmp(&a.conf).unwrap_or(std::cmp::Ordering::Equal));
    const TOPK: usize = 100;
    if raws.len() > TOPK {
        raws.truncate(TOPK);
    }
    // Class-agnostic NMS IoU 0.5 to remove duplicates before bbox decode.
    fn iou(a: &RawDet, b: &RawDet) -> f32 {
        let w = (a.x2.min(b.x2) - a.x1.max(b.x1)).max(0.0);
        let h = (a.y2.min(b.y2) - a.y1.max(b.y1)).max(0.0);
        let inter = w * h;
        let area_a = ((a.x2 - a.x1).max(0.0)) * ((a.y2 - a.y1).max(0.0));
        let area_b = ((b.x2 - b.x1).max(0.0)) * ((b.y2 - b.y1).max(0.0));
        if area_a + area_b - inter <= 0.0 { 0.0 } else { inter / (area_a + area_b - inter) }
    }
    const NMS_THRESH: f32 = 0.5;
    let mut kept: Vec<RawDet> = Vec::with_capacity(raws.len());
    'outer: for det in raws {
        for k in &kept {
            // Only suppress same class or high overlap across classes still suppresses duplicate SFX/balloon boxes
            if iou(&det, k) > NMS_THRESH {
                continue 'outer;
            }
        }
        kept.push(det);
    }
    // Box-only decode — mask branch deleted (was 400KiB * passes pure waste, mask: None never used by filter.rs).
    // Proto/mask_map computation removed until filter actually needs masks.
    let _ = (ph, pw, grid_h, grid_w, &proto_data, proto_chan_stride); // keep bindings for future mask feature
    for det in kept {
        let (x1, y1, x2, y2, conf, cls) = (det.x1, det.y1, det.x2, det.y2, det.conf, det.cls);
        let ox1 = (x1 - dx as f32) / r;
        let oy1 = (y1 - dy as f32) / r;
        let ox2 = (x2 - dx as f32) / r;
        let oy2 = (y2 - dy as f32) / r;
        let ox1c = (ox1.max(0.0)).round() as i32;
        let oy1c = (oy1.max(0.0)).round() as i32;
        let bbox = [ox1, oy1, ox2, oy2];
        if bbox[2] <= bbox[0] || bbox[3] <= bbox[1] {
            continue;
        }
        out.push(SegDet {
            class: SegClass::from_id(cls),
            class_id: cls,
            confidence: conf,
            bbox,
            mask: None,
            mask_origin: Some((ox1c, oy1c)),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seg_class_mapping() {
        assert_eq!(SegClass::from_id(0), SegClass::Frame);
        assert_eq!(SegClass::from_id(2), SegClass::Balloon);
        assert_eq!(SegClass::from_id(3), SegClass::Onomatopoeia);
        assert_eq!(SegClass::from_id(99), SegClass::Unknown);
    }

    #[test]
    fn letterbox_is_centered() {
        let img = RgbImage::from_pixel(800, 1200, image::Rgb([10, 20, 30]));
        let (canvas, r, dx, dy) = letterbox(&img, IMG_SIZE);
        assert_eq!(canvas.dimensions(), (IMG_SIZE, IMG_SIZE));
        // r = min(IMG_SIZE/800, IMG_SIZE/1200) = IMG_SIZE/1200
        let expected_r = IMG_SIZE as f32 / 1200.0;
        assert!((r - expected_r).abs() < 0.01);
        assert!(dx > 0);
        assert_eq!(dy, 0); // because height scales to IMG_SIZE, width to scaled_w, dx = (IMG_SIZE-scaled_w)/2
        // scaled_w = 800*expected_r rounded
        let expected_nw = (800 as f32 * expected_r).round() as u32;
        let expected_dx = ((IMG_SIZE - expected_nw) / 2) as i32;
        assert!((dx - expected_dx).abs() <= 1);
    }

    #[test]
    fn decode_empty_outputs_returns_empty() {
        let empty: Vec<ArrayD<f32>> = vec![];
        let dets = decode_koharu(&empty, 1.0, 0, 0, IMG_SIZE, IMG_SIZE);
        assert!(dets.is_empty());
    }
}
