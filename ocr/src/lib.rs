//! Wraps the rapidocr engine: configuration, lifecycle and conversion of raw
//! engine output into the model's append-only entries. The rest of the app
//! never touches rapidocr types directly.

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rapidocr_core::config::{
    DetConfig, InferenceOptions, LimitType, PipelineConfig, RapidOcrConfig, RecConfig,
};
use rapidocr_core::types::OcrLine;
use rapidocr_core::{is_cancelled_error, RapidOcr};

pub use rapidocr_core::OcrCancellationToken;

use scanlateit_model::{EntrySource, NewEntry, Quad};

const MODEL_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../models");

/// Cloneable handle to the shared OCR engine. Only one run may execute at a
/// time; runs are cancellable via [`OcrCancellationToken`].
#[derive(Clone)]
pub struct Engine(Arc<Mutex<RapidOcr>>);

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Engine")
    }
}

impl Engine {
    pub fn build() -> Result<Self, String> {
        RapidOcr::new(config())
            .map(|ocr| Self(Arc::new(Mutex::new(ocr))))
            .map_err(|e| format!("Engine init failed: {e}"))
    }

    /// Runs OCR on an image file. On cancel returns `Err("cancelled")`.
    pub fn run_path_cancellable(
        &self,
        path: &str,
        token: &OcrCancellationToken,
    ) -> Result<Vec<OcrLine>, String> {
        let mut engine = self
            .0
            .lock()
            .map_err(|e| format!("Engine lock poisoned: {e}"))?;
        engine
            .run_path_cancellable(path, token)
            .map(|output| output.lines)
            .map_err(|e| {
                if is_cancelled_error(&e) {
                    "cancelled".to_string()
                } else {
                    format!("OCR failed: {e}")
                }
            })
    }
}

fn config() -> RapidOcrConfig {
    let model_dir = PathBuf::from(MODEL_DIR);
    RapidOcrConfig {
        pipeline: PipelineConfig::without_cls(),
        inference: InferenceOptions {
            intra_threads: 4,
            ..Default::default()
        },
        text_score: 0.5,
        min_side_len: 30,
        max_side_len: 2000,
        min_height: 30,
        width_height_ratio: 8.0,
        det: Some(DetConfig {
            model_path: model_dir.join("PP-OCRv6_det_tiny.onnx"),
            limit_side_len: 736,
            limit_type: LimitType::Min,
            mean: [0.5, 0.5, 0.5],
            std: [0.5, 0.5, 0.5],
            thresh: 0.3,
            box_thresh: 0.5,
            max_candidates: 1000,
            unclip_ratio: 1.6,
            min_size: 3,
            input_limits: Default::default(),
        }),
        cls: None,
        rec: Some(RecConfig {
            model_path: model_dir.join("korean_PP-OCRv5_rec_mobile.onnx"),
            dict_path: model_dir.join("korean_dict.txt"),
            image_shape: [3, 48, 320],
            batch_size: 6,
        }),
    }
}

/// Tuning for merging nearby OCR text boxes into one entry.
///
/// Every margin is a ratio of the box's height, so the grouping is invariant
/// under image resolution changes: doubling the pixel dimensions doubles the
/// allowed gaps and the same lines still merge.
#[derive(Debug, Clone, Copy)]
pub struct MergeConfig {
    /// Side margin added to each box, as a ratio of its height.
    pub expand_x: f32,
    /// Top/bottom margin added to each box, as a ratio of its height.
    pub expand_y: f32,
}

impl Default for MergeConfig {
    fn default() -> Self {
        Self {
            expand_x: 0.5,
            expand_y: 0.5,
        }
    }
}

/// Convert raw engine output into model entries (auto-OCR source). Safe to
/// call from a background task; the model is untouched until appended.
///
/// Nearby text boxes are merged first (see [`MergeConfig::default`]).
pub fn to_entries(lines: Vec<OcrLine>) -> Vec<NewEntry> {
    to_entries_with(lines, MergeConfig::default())
}

/// Like [`to_entries`], but with explicit merge tuning.
pub fn to_entries_with(lines: Vec<OcrLine>, cfg: MergeConfig) -> Vec<NewEntry> {
    merge_lines(lines, cfg)
        .into_iter()
        .map(|line| NewEntry {
            source: EntrySource::AutoOcr,
            text: line.text,
            score: line.score,
            quad: Quad { points: line.bbox.points },
        })
        .collect()
}

/// AABBs of a detected text box as `[min_x, min_y, max_x, max_y]`.
fn box_bounds(quad: &rapidocr_core::types::Quad) -> [f32; 4] {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for point in &quad.points {
        min_x = min_x.min(point[0]);
        min_y = min_y.min(point[1]);
        max_x = max_x.max(point[0]);
        max_y = max_y.max(point[1]);
    }
    [min_x, min_y, max_x, max_y]
}

/// One cluster of merged lines: the union AABB plus the member lines.
#[derive(Default)]
struct Group {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    lines: Vec<OcrLine>,
}

impl Group {
    fn intersect(&self, other: [f32; 4]) -> bool {
        !(self.max_x < other[0]
            || self.min_x > other[2]
            || self.max_y < other[1]
            || self.min_y > other[3])
    }
}

/// Groups nearby OCR lines and emits one merged line per group.
///
/// Two boxes count as nearby when their AABBs intersect after each side is
/// expanded by a configurable fraction of its own height. Overlaps, small
/// gaps and near-distance boxes therefore all merge, and merging is
/// transitive: A near B and B near C also pulls A and C together. Merged
/// text is joined with single spaces in reading order (top-to-bottom, then
/// left-to-right); the merged score is the mean of its lines.
fn merge_lines(lines: Vec<OcrLine>, cfg: MergeConfig) -> Vec<OcrLine> {
    let mut lines = lines;
    lines.sort_by(|a, b| {
        let [ax0, ay0, _, ay1] = box_bounds(&a.bbox);
        let [bx0, by0, _, by1] = box_bounds(&b.bbox);
        let a_mid = (ay0 + ay1) / 2.0;
        let b_mid = (by0 + by1) / 2.0;
        a_mid.total_cmp(&b_mid).then(ax0.total_cmp(&bx0))
    });

    let mut groups: Vec<Group> = Vec::new();
    for line in lines {
        let [lx0, ly0, lx1, ly1] = box_bounds(&line.bbox);
        let height = (ly1 - ly0).max(1.0);
        let expanded = [
            lx0 - cfg.expand_x * height,
            ly0 - cfg.expand_y * height,
            lx1 + cfg.expand_x * height,
            ly1 + cfg.expand_y * height,
        ];

        let mut hits: Vec<usize> = groups
            .iter()
            .enumerate()
            .filter(|(_, group)| group.intersect(expanded))
            .map(|(index, _)| index)
            .collect();

        if hits.is_empty() {
            groups.push(Group {
                min_x: lx0,
                min_y: ly0,
                max_x: lx1,
                max_y: ly1,
                lines: vec![line],
            });
        } else {
            let first = hits.remove(0);
            let mut target = std::mem::take(&mut groups[first]);
            hits.sort_by(|a, b| b.cmp(a));
            for index in hits {
                let merged = groups.remove(index);
                target.min_x = target.min_x.min(merged.min_x);
                target.min_y = target.min_y.min(merged.min_y);
                target.max_x = target.max_x.max(merged.max_x);
                target.max_y = target.max_y.max(merged.max_y);
                target.lines.extend(merged.lines);
            }
            target.min_x = target.min_x.min(lx0);
            target.min_y = target.min_y.min(ly0);
            target.max_x = target.max_x.max(lx1);
            target.max_y = target.max_y.max(ly1);
            target.lines.push(line);
            groups[first] = target;
        }
    }

    groups
        .into_iter()
        .map(|group| {
            let text = group
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let score = group
                .lines
                .iter()
                .map(|line| line.score)
                .sum::<f32>()
                / group.lines.len().max(1) as f32;
            OcrLine {
                bbox: rapidocr_core::types::Quad::from_xyxy(
                    group.min_x, group.min_y, group.max_x, group.max_y,
                ),
                text,
                score,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, x0: f32, y0: f32, x1: f32, y1: f32, score: f32) -> OcrLine {
        OcrLine {
            bbox: rapidocr_core::types::Quad::from_xyxy(x0, y0, x1, y1),
            text: text.to_string(),
            score,
        }
    }

    #[test]
    fn merges_gapped_lines_into_one_entry() {
        let lines = vec![
            line("first", 10.0, 10.0, 60.0, 30.0, 0.9),
            line("second", 65.0, 10.0, 120.0, 30.0, 0.7),
        ];
        let entries = to_entries(lines);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "first second");
        assert!((entries[0].score - 0.8).abs() < 1e-6);
        let [min_x, min_y, max_x, max_y] = entries[0].quad.bounds();
        assert_eq!([min_x, min_y, max_x, max_y], [10.0, 10.0, 120.0, 30.0]);
    }

    #[test]
    fn keeps_far_apart_boxes_separate() {
        let lines = vec![
            line("first", 10.0, 10.0, 60.0, 30.0, 0.9),
            line("second", 200.0, 10.0, 260.0, 30.0, 0.7),
        ];
        let entries = to_entries(lines);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "first");
        assert_eq!(entries[1].text, "second");
    }

    #[test]
    fn merging_is_transitive() {
        let lines = vec![
            line("a", 10.0, 10.0, 40.0, 30.0, 0.9),
            line("c", 80.0, 10.0, 110.0, 30.0, 0.7),
            line("b", 45.0, 10.0, 75.0, 30.0, 0.8),
        ];
        let entries = to_entries(lines);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "a b c");
    }

    #[test]
    fn grouping_is_resolution_invariant() {
        let low = vec![
            line("first", 10.0, 10.0, 60.0, 30.0, 0.9),
            line("second", 65.0, 10.0, 120.0, 30.0, 0.7),
            line("far", 200.0, 10.0, 260.0, 30.0, 0.8),
        ];
        let high = vec![
            line("first", 20.0, 20.0, 120.0, 60.0, 0.9),
            line("second", 130.0, 20.0, 240.0, 60.0, 0.7),
            line("far", 400.0, 20.0, 520.0, 60.0, 0.8),
        ];
        let low_groups: Vec<String> = to_entries(low)
            .into_iter()
            .map(|e| e.text)
            .collect();
        let high_groups: Vec<String> = to_entries(high)
            .into_iter()
            .map(|e| e.text)
            .collect();
        assert_eq!(low_groups, high_groups);
        assert_eq!(low_groups, vec!["first second".to_string(), "far".to_string()]);
    }

    #[test]
    fn zero_expansion_merges_only_touching_or_overlapping() {
        let cfg = MergeConfig {
            expand_x: 0.0,
            expand_y: 0.0,
        };
        let touching = vec![
            line("a", 10.0, 10.0, 60.0, 30.0, 0.9),
            line("b", 60.0, 10.0, 120.0, 30.0, 0.7),
        ];
        assert_eq!(to_entries_with(touching, cfg).len(), 1);

        let gapped = vec![
            line("a", 10.0, 10.0, 60.0, 30.0, 0.9),
            line("b", 65.0, 10.0, 120.0, 30.0, 0.7),
        ];
        assert_eq!(to_entries_with(gapped, cfg).len(), 2);
    }

    #[test]
    fn empty_lines_yield_no_entries() {
        assert!(to_entries(vec![]).is_empty());
    }
}