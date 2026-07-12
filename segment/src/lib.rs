//! Segmentation (panel / balloon / SFX) inference for manga-mimic grids.
//!
//! Wraps `koharu-yolo26s-1280.onnx` (classes: frame, dialogue_text, balloon,
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
const MODEL_FILE_KOHARU: &str = "koharu-yolo26s-1280.onnx";
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
        let koharu = Path::new(MODEL_DIR).join(MODEL_FILE_KOHARU);
        let fallback = Path::new(MODEL_DIR).join(MODEL_FILE_FALLBACK);
        let alt = Path::new(MODEL_DIR)
            .join("../onnx-text-styling-classification/panel-bubble-sfx-det")
            .join(MODEL_FILE_KOHARU);
        let path = if koharu.exists() {
            koharu
        } else if fallback.exists() {
            fallback
        } else if alt.exists() {
            alt
        } else {
            return Err(format!(
                "segmentation model not found: tried {}, {} and {}",
                koharu.display(),
                fallback.display(),
                alt.display()
            ));
        };
        let session = Session::builder()
            .map_err(|e| format!("ORT init failed: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| format!("ORT init failed: {e}"))?
            .with_intra_threads(2)
            .map_err(|e| format!("ORT init failed: {e}"))?
            .with_execution_providers([ort::ep::CPU::default().build()])
            .map_err(|e| format!("ORT init failed: {e}"))?
            .commit_from_file(&path)
            .map_err(|e| format!("failed to load segmentation model {}: {e}", path.display()))?;
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
    orig_w: u32,
    orig_h: u32,
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
    // Iterate detections
    for i in 0..num_det {
        // Index in flattened det_data: need to account for batch=1 case where layout is [1,300,38] row-major => offset = i*38
        // For shape [300,38] same.
        let base = i * det_dim;
        if base + 6 > det_data.len() {
            break;
        }
        let x1 = det_data[base];
        let y1 = det_data[base + 1];
        let x2 = det_data[base + 2];
        let y2 = det_data[base + 3];
        let conf = det_data[base + 4];
        let cls_f = det_data[base + 5];
        if conf <= conf_thres {
            continue;
        }
        let cls = cls_f as usize;
        if cls >= CLASSES.len() {
            continue;
        }
        // Coeffs 32
        let coeffs: Vec<f32> = if det_dim >= 38 {
            det_data[base + 6..base + 38].to_vec()
        } else {
            vec![0.0; 32]
        };
        // Build mask_map = sigmoid(coeffs @ proto)
        // proto_data layout: [32, ph, pw] contiguous channel-major
        let mut mask_map = vec![0.0f32; (ph * pw) as usize];
        for c in 0..32.min(coeffs.len()) {
            let coeff = coeffs[c];
            let offset = c * proto_chan_stride;
            for p in 0..proto_chan_stride {
                // proto_data may have batch dim; if shape len 4, data is batch 0 then channels, same.
                mask_map[p] += coeff * proto_data.get(offset + p).copied().unwrap_or(0.0);
            }
        }
        for v in &mut mask_map {
            *v = sigmoid(*v);
        }
        // Crop mask to box in proto grid
        let bx1 = (x1 * grid_w / IMG_SIZE as f32) as i32;
        let by1 = (y1 * grid_h / IMG_SIZE as f32) as i32;
        let bx2 = (x2 * grid_w / IMG_SIZE as f32) as i32;
        let by2 = (y2 * grid_h / IMG_SIZE as f32) as i32;
        let bx1c = bx1.max(0);
        let by1c = by1.max(0);
        let mut bw = (bx2 - bx1c).max(1);
        let mut bh = (by2 - by1c).max(1);
        if by1c >= ph || bx1c >= pw {
            continue;
        }
        bh = bh.min(ph - by1c);
        bw = bw.min(pw - bx1c);
        if bh <= 0 || bw <= 0 {
            continue;
        }
        // Extract crop (not used for box filter but for mask)
        // For now we skip detailed mask resize; just store box.
        let ox1 = (x1 - dx as f32) / r;
        let oy1 = (y1 - dy as f32) / r;
        let ox2 = (x2 - dx as f32) / r;
        let oy2 = (y2 - dy as f32) / r;
        let ox1c = (ox1.max(0.0)).round() as i32;
        let oy1c = (oy1.max(0.0)).round() as i32;
        let ox2c = (ox2.min(orig_w as f32)).round() as i32;
        let oy2c = (oy2.min(orig_h as f32)).round() as i32;
        let _bw_o = ((ox2 - ox1).round() as i32).max(1);
        let _bh_o = ((oy2 - oy1).round() as i32).max(1);
        // We do not fully reconstruct mask_bin here for filter (box-only). Keep bbox.
        let bbox = [ox1, oy1, ox2, oy2];
        // Filter degenerate
        if bbox[2] <= bbox[0] || bbox[3] <= bbox[1] {
            continue;
        }
        // For mask, we could store cropped mask but not needed for box filter.
        out.push(SegDet {
            class: SegClass::from_id(cls),
            class_id: cls,
            confidence: conf,
            bbox,
            mask: None,
            mask_origin: Some((ox1c, oy1c)),
        });
        let _ = (ox2c, oy2c, bx1c, by1c, bw, bh); // silence unused
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
        let (canvas, r, dx, dy) = letterbox(&img, 1280);
        assert_eq!(canvas.dimensions(), (1280, 1280));
        // r = min(1280/800=1.6, 1280/1200=1.066) =1.066
        assert!((r - 1.0666).abs() < 0.01);
        assert!(dx > 0);
        assert_eq!(dy, 0); // because height scales to 1280, width to 853, dx = (1280-853)/2≈213
        assert!(dx > 200 && dx < 220);
    }

    #[test]
    fn decode_empty_outputs_returns_empty() {
        let empty: Vec<ArrayD<f32>> = vec![];
        let dets = decode_koharu(&empty, 1.0, 0, 0, 1280, 1280);
        assert!(dets.is_empty());
    }
}
