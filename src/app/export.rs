use std::cell::RefCell;
use std::path::{Path as FsPath, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use iced::advanced::graphics::geometry::{self, Fill, Stroke, Text};
use iced::advanced::graphics::geometry::Path as GeomPath;
use iced::futures::{SinkExt, StreamExt};
use iced::{Font, Point, Rectangle, Size, Vector, Radians};
use image::RgbaImage;
use rayon::prelude::*;
use tiny_skia::{FillRule, Mask, Paint, Pixmap, PremultipliedColorU8, Rect, Shader, Transform};

use easyscanlate_model::Project;
use easyscanlate_ui::main_area::overlay::{self, OverlayEntry};

use super::{App, Message};
use iced::Task;

// ---------------------------------------------------------------------------
// tiny-skia helpers — copied from iced_tiny_skia::geometry
// ---------------------------------------------------------------------------
fn convert_path(path: &GeomPath) -> Option<tiny_skia::Path> {
    use iced::advanced::graphics::geometry::path::lyon_path;
    let mut builder = tiny_skia::PathBuilder::new();
    let mut last_point = lyon_path::math::Point::default();
    for event in path.raw() {
        match event {
            lyon_path::Event::Begin { at } => {
                builder.move_to(at.x, at.y);
                last_point = at;
            }
            lyon_path::Event::Line { from, to } => {
                if last_point != from {
                    builder.move_to(from.x, from.y);
                }
                builder.line_to(to.x, to.y);
                last_point = to;
            }
            lyon_path::Event::Quadratic { from, ctrl, to } => {
                if last_point != from {
                    builder.move_to(from.x, from.y);
                }
                builder.quad_to(ctrl.x, ctrl.y, to.x, to.y);
                last_point = to;
            }
            lyon_path::Event::Cubic { from, ctrl1, ctrl2, to } => {
                if last_point != from {
                    builder.move_to(from.x, from.y);
                }
                builder.cubic_to(ctrl1.x, ctrl1.y, ctrl2.x, ctrl2.y, to.x, to.y);
                last_point = to;
            }
            lyon_path::Event::End { close, .. } => {
                if close {
                    builder.close();
                }
            }
        }
    }
    builder.finish()
}

fn into_fill_rule(rule: geometry::fill::Rule) -> FillRule {
    match rule {
        geometry::fill::Rule::EvenOdd => FillRule::EvenOdd,
        geometry::fill::Rule::NonZero => FillRule::Winding,
    }
}

fn into_paint(style: geometry::Style) -> Paint<'static> {
    use iced::advanced::graphics::geometry::Style as GStyle;
    Paint {
        shader: match style {
            GStyle::Solid(color) => Shader::SolidColor(
                tiny_skia::Color::from_rgba(color.r, color.g, color.b, color.a)
                    .expect("valid color"),
            ),
            GStyle::Gradient(grad) => {
                // Simplified: treat any gradient as solid first stop (export always uses per-glyph solid for gradients)
                let solid = match grad {
                    iced::advanced::graphics::Gradient::Linear(l) => l
                        .stops
                        .into_iter()
                        .flatten()
                        .next()
                        .map(|s| s.color)
                        .unwrap_or(iced::Color::BLACK),
                };
                Shader::SolidColor(
                    tiny_skia::Color::from_rgba(solid.b, solid.g, solid.r, solid.a)
                        .expect("valid color"),
                )
            }
        },
        anti_alias: true,
        ..Default::default()
    }
}

fn into_stroke(stroke: &Stroke<'_>) -> tiny_skia::Stroke {
    tiny_skia::Stroke {
        width: stroke.width,
        line_cap: match stroke.line_cap {
            geometry::stroke::LineCap::Butt => tiny_skia::LineCap::Butt,
            geometry::stroke::LineCap::Square => tiny_skia::LineCap::Square,
            geometry::stroke::LineCap::Round => tiny_skia::LineCap::Round,
        },
        line_join: match stroke.line_join {
            geometry::stroke::LineJoin::Miter => tiny_skia::LineJoin::Miter,
            geometry::stroke::LineJoin::Round => tiny_skia::LineJoin::Round,
            geometry::stroke::LineJoin::Bevel => tiny_skia::LineJoin::Bevel,
        },
        dash: if stroke.line_dash.segments.is_empty() {
            None
        } else {
            tiny_skia::StrokeDash::new(stroke.line_dash.segments.into(), stroke.line_dash.offset as f32)
        },
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// ExportFrame — immediate-mode Backend that draws onto a tiny-skia Pixmap
// ---------------------------------------------------------------------------
struct ExportFrame {
    pixmap: Rc<RefCell<Pixmap>>,
    transform: Transform,
    stack: Vec<Transform>,
    clip: Option<Rectangle>,
    clip_stack: Vec<Option<Rectangle>>,
    // Single-entry cache for the full-size clip mask. Gradient text draws
    // every glyph once per band with the same clip, so rebuilding
    // `Mask::new(w,h)` per glyph was the dominant raster cost. Same bits,
    // just reused (bit-identical output).
    mask_cache: RefCell<Option<(u32, u32, u32, u32, u32, u32, Mask)>>,
}

impl ExportFrame {
    fn new(pixmap: Pixmap) -> Self {
        Self {
            pixmap: Rc::new(RefCell::new(pixmap)),
            transform: Transform::identity(),
            stack: Vec::new(),
            clip: None,
            clip_stack: Vec::new(),
            mask_cache: RefCell::new(None),
        }
    }

    fn into_pixmap(self) -> Pixmap {
        // should be sole owner (all drafts dropped)
        Rc::try_unwrap(self.pixmap)
            .expect("ExportFrame leaked")
            .into_inner()
    }

    fn clip_mask(&self, w: u32, h: u32) -> Option<Mask> {
        let clip = self.clip?;
        // clip is in frame coords (image pixels). For immediate we treat it as pixmap coords.
        // Clamp to pixmap bounds
        let x = clip.x.max(0.0);
        let y = clip.y.max(0.0);
        let rw = (clip.width).min(w as f32 - x).max(0.0);
        let rh = (clip.height).min(h as f32 - y).max(0.0);
        let key = (w, h, x.to_bits(), y.to_bits(), rw.to_bits(), rh.to_bits());
        if let Some((kw, kh, kx, ky, krw, krh, cached)) = self.mask_cache.borrow().as_ref()
            && (*kw, *kh, *kx, *ky, *krw, *krh) == key
        {
            return Some(cached.clone());
        }
        let built = (|| {
            if rw <= 0.0 || rh <= 0.0 {
                // empty clip -> fully masked (nothing draws)
                let m = Mask::new(w, h)?;
                // leave all zero
                return Some(m);
            }
            let mut mask = Mask::new(w, h)?;
            let rect = Rect::from_xywh(x, y, rw, rh)?;
            let path = {
                let mut b = tiny_skia::PathBuilder::new();
                b.push_rect(rect);
                b.finish()?
            };
            mask.fill_path(&path, FillRule::Winding, false, Transform::identity());
            Some(mask)
        })()?;
        *self.mask_cache.borrow_mut() = Some((key.0, key.1, key.2, key.3, key.4, key.5, built.clone()));
        Some(built)
    }
}

impl geometry::frame::Backend for ExportFrame {
    type Geometry = ();

    fn width(&self) -> f32 {
        self.pixmap.borrow().width() as f32
    }
    fn height(&self) -> f32 {
        self.pixmap.borrow().height() as f32
    }
    fn size(&self) -> Size {
        let p = self.pixmap.borrow();
        Size::new(p.width() as f32, p.height() as f32)
    }
    fn center(&self) -> Point {
        let p = self.pixmap.borrow();
        Point::new(p.width() as f32 / 2.0, p.height() as f32 / 2.0)
    }

    fn push_transform(&mut self) {
        self.stack.push(self.transform);
        self.clip_stack.push(self.clip);
    }
    fn pop_transform(&mut self) {
        self.transform = self.stack.pop().expect("pop_transform");
        self.clip = self.clip_stack.pop().expect("pop clip");
    }

    fn translate(&mut self, v: Vector) {
        self.transform = self.transform.pre_translate(v.x, v.y);
    }
    fn rotate(&mut self, angle: impl Into<Radians>) {
        let rad: f32 = angle.into().0;
        self.transform = self.transform.pre_concat(Transform::from_rotate(rad.to_degrees()));
    }
    fn scale(&mut self, s: impl Into<f32>) {
        let s = s.into();
        self.transform = self.transform.pre_scale(s, s);
    }
    fn scale_nonuniform(&mut self, s: impl Into<Vector>) {
        let s = s.into();
        self.transform = self.transform.pre_scale(s.x, s.y);
    }

    fn draft(&mut self, clip_bounds: Rectangle) -> Self {
        let new_clip = match self.clip {
            Some(parent) => parent.intersection(&clip_bounds).unwrap_or(Rectangle::with_size(Size::ZERO)),
            None => clip_bounds,
        };
        Self {
            pixmap: Rc::clone(&self.pixmap),
            transform: self.transform,
            stack: self.stack.clone(),
            clip: Some(new_clip),
            clip_stack: self.clip_stack.clone(),
            mask_cache: RefCell::new(None),
        }
    }
    fn paste(&mut self, _frame: Self) {
        // draft shares pixmap, draws already applied with its clip
    }

    fn fill(&mut self, path: &GeomPath, fill: impl Into<Fill>) {
        let fill = fill.into();
        let sk_path = match convert_path(path) {
            Some(p) => match p.transform(self.transform) {
                Some(tp) => tp,
                None => return,
            },
            None => return,
        };
        let mut paint = into_paint(fill.style);
        paint.shader.transform(self.transform);
        let rule = into_fill_rule(fill.rule);
        let mut pix = self.pixmap.borrow_mut();
        let mask = self.clip_mask(pix.width(), pix.height());
        pix.fill_path(&sk_path, &paint, rule, Transform::identity(), mask.as_ref());
    }

    fn fill_rectangle(&mut self, top_left: Point, size: Size, fill: impl Into<Fill>) {
        self.fill(&GeomPath::rectangle(top_left, size), fill);
    }

    fn stroke<'a>(&mut self, path: &GeomPath, stroke: impl Into<Stroke<'a>>) {
        let stroke = stroke.into();
        let sk_path = match convert_path(path) {
            Some(p) => match p.transform(self.transform) {
                Some(tp) => tp,
                None => return,
            },
            None => return,
        };
        let mut paint = into_paint(stroke.style);
        paint.shader.transform(self.transform);
        let sk_stroke = into_stroke(&stroke);
        let mut pix = self.pixmap.borrow_mut();
        let mask = self.clip_mask(pix.width(), pix.height());
        pix.stroke_path(&sk_path, &paint, &sk_stroke, Transform::identity(), mask.as_ref());
    }

    fn stroke_rectangle<'a>(&mut self, top_left: Point, size: Size, stroke: impl Into<Stroke<'a>>) {
        self.stroke(&GeomPath::rectangle(top_left, size), stroke);
    }

    fn fill_text(&mut self, text: impl Into<Text>) {
        let text = text.into();
        // Use raw pointer to avoid double &mut borrow inside draw_with closure
        let self_ptr = self as *mut Self;
        text.draw_with(|path, color| unsafe {
            (*self_ptr).fill(&path, color);
        });
    }

    fn stroke_text<'a>(&mut self, text: impl Into<Text>, stroke: impl Into<Stroke<'a>>) {
        let text = text.into();
        let stroke = stroke.into();
        let self_ptr = self as *mut Self;
        // need to clone stroke per glyph
        text.draw_with(|path, _| unsafe {
            (*self_ptr).stroke(&path, stroke);
        });
    }

    fn draw_image(&mut self, _bounds: Rectangle, _image: impl Into<geometry::Image>) {
        // overlays don't use draw_image; inpaint/base already composited via image crate
    }
    fn draw_svg(&mut self, _bounds: Rectangle, _svg: impl Into<geometry::Svg>) {}

    fn into_geometry(self) -> Self::Geometry {}
}

// ---------------------------------------------------------------------------
// Helpers: Pixmap <-> RgbaImage (bit-identical, parallel)
// ---------------------------------------------------------------------------
fn rgba_to_pixmap(img: &RgbaImage) -> Pixmap {
    let (w, h) = img.dimensions();
    let mut pix = Pixmap::new(w, h).unwrap_or_else(|| Pixmap::new(1, 1).unwrap());
    // tiny-skia expects premultiplied; image crate stores straight.
    // Same per-pixel math as before (`from_rgba` + transparent fallback),
    // just spread across cores. Output bytes are identical.
    let raw = img.as_raw();
    pix.pixels_mut()
        .par_iter_mut()
        .enumerate()
        .for_each(|(i, dst)| {
            let o = i * 4;
            let (r, g, b, a) = (raw[o], raw[o + 1], raw[o + 2], raw[o + 3]);
            // premultiply
            *dst = PremultipliedColorU8::from_rgba(r, g, b, a)
                .unwrap_or(PremultipliedColorU8::TRANSPARENT);
        });
    pix
}

fn pixmap_to_rgba(pix: Pixmap) -> RgbaImage {
    let w = pix.width();
    let h = pix.height();
    // unpremultiply to straight for image crate png/jpeg — same `demultiply`
    // math as before, parallelized over rows. Identical bytes.
    let src = pix.pixels();
    let mut data = vec![0u8; src.len() * 4];
    data.par_chunks_mut(4)
        .enumerate()
        .for_each(|(i, chunk)| {
            let c = src[i].demultiply();
            chunk[0] = c.red();
            chunk[1] = c.green();
            chunk[2] = c.blue();
            chunk[3] = c.alpha();
        });
    RgbaImage::from_raw(w, h, data).expect("pixmap size")
}

// ---------------------------------------------------------------------------
// Per-page rasterization
// ---------------------------------------------------------------------------
fn handle_to_rgba(handle: &iced::widget::image::Handle) -> Option<RgbaImage> {
    match handle {
        iced::widget::image::Handle::Rgba { width, height, pixels, .. } => {
            let w = *width;
            let h = *height;
            let data = pixels.to_vec();
            if data.len() as u32 == w * h * 4 {
                RgbaImage::from_raw(w, h, data)
            } else {
                None
            }
        }
        iced::widget::image::Handle::Bytes(_, bytes) => {
            image::load_from_memory(bytes).ok().map(|i| i.to_rgba8())
        }
        _ => None,
    }
}

#[allow(dead_code)]
fn rasterize_page(
    base: RgbaImage,
    inpaint_raw: &[( [f32;4], RgbaImage )],
    project: &Project,
    image_id: easyscanlate_model::ImageId,
    font: Font,
) -> RgbaImage {
    let snapshot = ExportSnapshot::build(project);
    rasterize_page_with_snapshot(base, inpaint_raw, &snapshot, image_id, font)
}

// Precomputed per-export snapshot so 90–200 pages don't rebuild
// `global_offsets` + `owner_to_idx` + visible entry list per page.
// Same predicates/order as before — identical output, O(P+E) build.
#[derive(Clone)]
struct SnapshotEntry {
    id: easyscanlate_model::EntryId,
    image_id: easyscanlate_model::ImageId,
    quad: easyscanlate_model::Quad,
    text: String,
    style: easyscanlate_model::EntryStyle,
}

#[derive(Clone)]
pub(crate) struct ExportSnapshot {
    global_offsets: Vec<f32>,
    owner_to_idx: std::collections::HashMap<easyscanlate_model::ImageId, usize>,
    owner_widths: Vec<f32>,
    page_ids: Vec<easyscanlate_model::ImageId>,
    entries: Vec<SnapshotEntry>,
}

impl ExportSnapshot {
    fn build(project: &Project) -> Self {
        let images = project.images();
        let mut global_offsets = Vec::with_capacity(images.len());
        let mut cur = 0.0f32;
        for m in images {
            global_offsets.push(cur);
            cur += m.height;
        }
        let mut owner_to_idx = std::collections::HashMap::with_capacity(images.len() * 2);
        let mut owner_widths = Vec::with_capacity(images.len());
        let mut page_ids = Vec::with_capacity(images.len());
        for (i, m) in images.iter().enumerate() {
            owner_to_idx.insert(m.id, i);
            owner_widths.push(m.width);
            page_ids.push(m.id);
        }
        let mut entries = Vec::new();
        for e in project.visible_entries() {
            entries.push(SnapshotEntry {
                id: e.id,
                image_id: e.image_id,
                quad: project.view_quad(e),
                text: project.display_text(e).to_string(),
                style: project.entry_style(e.id),
            });
        }
        Self { global_offsets, owner_to_idx, owner_widths, page_ids, entries }
    }
}

fn rasterize_page_with_snapshot(
    base: RgbaImage,
    inpaint_raw: &[( [f32;4], RgbaImage )],
    snapshot: &ExportSnapshot,
    image_id: easyscanlate_model::ImageId,
    font: Font,
) -> RgbaImage {
    // 1. composite inpaint onto base (image crate)
    let mut out = base;
    for (bounds, patch) in inpaint_raw {
        let x = bounds[0].round() as i64;
        let y = bounds[1].round() as i64;
        // overlay handles bounds larger than patch? In model bounds is [x,y,w,h] and patch dimensions match w/h
        image::imageops::overlay(&mut out, patch, x, y);
    }

    // 2. draw overlays via ExportFrame
    let (w, h) = out.dimensions();
    if w == 0 || h == 0 {
        return out;
    }
    let mut pix = rgba_to_pixmap(&out);

    // Build OverlayEntry list (always baked, ignore UI toggles)
    // Keep owned strings alive
    // Global stitched canvas: each image stacked vertically using meta heights (matches viewer tile_layout).
    // An entry whose view_quad is beyond its owning image (y<0 or y>h) or that visually spans a seam
    // in the viewer will have a global y that intersects the neighboring page. We therefore render
    // *any* visible entry whose global bounds intersect this page's global interval, translating
    // its quad to this page's local pixel space. This fixes both "span 2 images" and "beyond" cases.
    let page_idx = snapshot.page_ids.iter().position(|id| *id == image_id).unwrap_or(0);
    let page_g0 = snapshot.global_offsets.get(page_idx).copied().unwrap_or(0.0);
    let page_g1 = page_g0 + h as f32;

    let mut texts: Vec<String> = Vec::new();
    let mut metas: Vec<(easyscanlate_model::EntryId, easyscanlate_model::Quad, easyscanlate_model::EntryStyle)> = Vec::new();

    for e in &snapshot.entries {
        let orig_quad = e.quad;
        let [vx0, vy0, vx1, vy1] = orig_quad.bounds();
        let owner_idx = match snapshot.owner_to_idx.get(&e.image_id) {
            Some(v) => *v,
            None => continue,
        };
        let owner_g0 = snapshot.global_offsets.get(owner_idx).copied().unwrap_or(0.0);
        let owner_w = snapshot.owner_widths.get(owner_idx).copied().unwrap_or(w as f32);
        let scale_x = if owner_w > 1.0 && (owner_w - w as f32).abs() > 0.5 { w as f32 / owner_w } else { 1.0 };
        // global bounds (x stays local unless width differs)
        let gx0 = vx0 * scale_x;
        let gx1 = vx1 * scale_x;
        let gy0 = owner_g0 + vy0;
        let gy1 = owner_g0 + vy1;
        let intersects = !(gx1 <= 0.0 || gx0 >= w as f32 || gy1 <= page_g0 || gy0 >= page_g1);
        if !intersects {
            continue;
        }
        // Map to target local space
        let dy = owner_g0 - page_g0;
        let mut q = orig_quad;
        for p in &mut q.points {
            p[0] *= scale_x;
            p[1] += dy;
        }
        texts.push(e.text.clone());
        metas.push((e.id, q, e.style.clone()));
    }
    let mut entries: Vec<OverlayEntry<'_>> = Vec::with_capacity(metas.len());
    for (i, (id, quad, style)) in metas.iter().enumerate() {
        entries.push(OverlayEntry {
            id: *id,
            text: &texts[i],
            quad: *quad,
            bounds: quad.bounds(),
            style: style.clone(),
            selected: false,
            quad_overridden: false,
            hide_text: false,
        });
    }

    if !entries.is_empty() {
        let mut frame = ExportFrame::new(pix);
        // scale = frame.width / image_width, here 1.0 (already mapped to target local)
        overlay::draw_entries(&mut frame, &entries, font, w as f32, false);
        pix = frame.into_pixmap();
    }

    pixmap_to_rgba(pix)
}

// ---------------------------------------------------------------------------
// Public handlers
// ---------------------------------------------------------------------------
pub fn handle_export_all(app: &mut App) -> Task<Message> {
    if app.active_tab().project.images().is_empty() || app.active_tab().images.is_empty() {
        app.active_tab_mut().status = "Nothing to export.".to_string();
        return Task::none();
    }
    // default folder = mmtl parent or first image parent
    let tab = app.active_tab();
    let tid = tab.id;
    let default_dir = tab
        .mmtl_path
        .as_ref()
        .and_then(|p| p.parent().map(|par| par.to_path_buf()))
        .or_else(|| {
            tab.project
                .images()
                .first()
                .map(|m| std::path::Path::new(&m.path).parent().map(|p| p.to_path_buf()).unwrap_or_default())
        })
        .unwrap_or_default();

    Task::perform(
        async move {
            let mut dlg = rfd::AsyncFileDialog::new();
            if default_dir.exists() {
                dlg = dlg.set_directory(&default_dir);
            }
            let folder = dlg.pick_folder().await;
            folder.map(|f| f.path().to_string_lossy().to_string())
        },
        move |picked| Message::Tab(tid, crate::app::TabMessage::ExportFolderPicked(picked)),
    )
}

/// Stashed raster-export job while the clean-base screenshot is captured for
/// the progress overlay. Started on `BackdropReady(Export)` via
/// `start_pending_export` so the capture never contains the overlay.
pub(crate) struct PendingExport {
    tab_id: crate::app::tab::TabId,
    units: Vec<ExportUnit>,
    snapshot: Arc<ExportSnapshot>,
    folder_path: PathBuf,
    font: Font,
}

pub fn handle_export_picked(app: &mut App, tab_id: crate::app::tab::TabId, folder: Option<String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    let Some(folder_str) = folder else {
        app.tabs[idx].status = "Export cancelled.".to_string();
        return Task::none();
    };
    let folder_path = PathBuf::from(&folder_str);
    if !folder_path.exists()
        && let Err(e) = std::fs::create_dir_all(&folder_path) {
            app.tabs[idx].status = format!("Export failed: cannot create folder: {e}");
            return Task::none();
        }

    if app.tabs[idx].exporting {
        app.tabs[idx].status = "Export already running...".to_string();
        return Task::none();
    }

    // Snapshot project + inpaint raw for background threads
    let project = app.tabs[idx].project.clone();
    let font = app.font.unwrap_or(Font::DEFAULT);

    // Extract inpaint raw per image (bounds + RgbaImage) to avoid Handle Send issues
    let mut inpaint_per_image: Vec<Vec<([f32; 4], RgbaImage)>> = Vec::new();
    for loaded in &app.tabs[idx].images {
        let mut v = Vec::new();
        for layer in &loaded.inpaint {
            if let Some(rgba) = handle_to_rgba(&layer.handle) {
                v.push((layer.bounds, rgba));
            }
        }
        inpaint_per_image.push(v);
    }

    // Also need original paths and ids
    let metas: Vec<(easyscanlate_model::ImageId, String)> = project
        .images()
        .iter()
        .map(|m| (m.id, m.path.clone()))
        .collect();

    if metas.is_empty() {
        app.tabs[idx].status = "Nothing to export.".to_string();
        return Task::none();
    }

    // Precompute deterministic output paths sequentially (same dedup as before),
    // reserving this batch's paths so parallel workers can't collide.
    let mut reserved: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut units: Vec<ExportUnit> = Vec::with_capacity(metas.len());
    for (job_idx, (image_id, src_path)) in metas.iter().enumerate() {
        let (out_path, ext) = resolve_out_path(&folder_path, src_path, *image_id, &reserved);
        reserved.insert(out_path.clone());
        let inpaint = inpaint_per_image.get(job_idx).cloned().unwrap_or_default();
        units.push(ExportUnit { idx: job_idx, image_id: *image_id, src_path: src_path.clone(), out_path, ext, inpaint });
    }

    let snapshot = Arc::new(ExportSnapshot::build(&project));
    // Drop the cloned project early; workers only need the snapshot.
    drop(project);

    let pending = PendingExport { tab_id, units, snapshot, folder_path, font };
    // Capture the clean base for the blurred overlay, then start on
    // `BackdropReady`. Falls back to an immediate flat start when headless.
    super::backdrop::begin_export(app, pending)
}

/// Starts a stashed export (immediate fallback or `BackdropReady` replay).
/// Sets the `exporting` counters + cancel flag and spawns the parallel
/// chunk stream. Late chunks after cancel are ignored by
/// `handle_export_stream_run`'s `!exporting` guard plus the atomic check.
pub(crate) fn start_pending_export(app: &mut App, op: PendingExport) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == op.tab_id) {
        Some(i) => i,
        None => {
            // Tab closed while capturing: drop the job and its blur.
            app.pending_export = None;
            app.export_blur = None;
            return Task::none();
        }
    };
    if app.tabs[idx].exporting {
        app.tabs[idx].status = "Export already running...".to_string();
        app.export_blur = None;
        return Task::none();
    }
    let total = op.units.len();
    if total == 0 {
        app.tabs[idx].status = "Nothing to export.".to_string();
        app.export_blur = None;
        return Task::none();
    }
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    app.tabs[idx].exporting = true;
    app.tabs[idx].export_total = total;
    app.tabs[idx].export_done = 0;
    app.tabs[idx].export_failed = 0;
    app.tabs[idx].export_errors = Vec::new();
    app.tabs[idx].export_folder = Some(op.folder_path.clone());
    app.tabs[idx].export_cancel = Some(Arc::clone(&cancel));
    app.tabs[idx].status = format!("Exporting 0 of {total} image(s)...");

    let PendingExport { tab_id: tid, units, snapshot, font, .. } = op;
    let chunk_size = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 16);
    Task::stream(
        iced::stream::try_channel(1, move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
            let mut units = units;
            while !units.is_empty() {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return Ok::<(), String>(());
                }
                let n = chunk_size.min(units.len());
                let chunk: Vec<ExportUnit> = units.drain(..n).collect();
                let snap = Arc::clone(&snapshot);
                let res: super::ExportStreamItem =
                    tokio::task::spawn_blocking(move || export_chunk_parallel(chunk, &snap, font))
                        .await
                        .unwrap_or_else(|e| vec![(usize::MAX, Err(format!("export task failed: {e}")))]);
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return Ok::<(), String>(());
                }
                if sender
                    .send(Message::Tab(tid, crate::app::TabMessage::ExportStreamRun(Ok(res))))
                    .await
                    .is_err()
                {
                    return Ok::<(), String>(());
                }
            }
            Ok::<(), String>(())
        })
        .map(move |item: Result<Message, String>| match item {
            Ok(message) => message,
            Err(e) => Message::Tab(tid, crate::app::TabMessage::ExportStreamFailed(e.to_string())),
        }),
    )
}

/// Cancels the running export for the active tab. Sets the atomic flag so
/// the stream loop stops after the current chunk; late messages are ignored.
pub fn handle_export_cancel(app: &mut App) -> Task<Message> {
    let idx = match app.tabs.get(app.active) {
        Some(_) => app.active,
        None => return Task::none(),
    };
    if !app.tabs[idx].exporting {
        return Task::none();
    }
    if let Some(flag) = app.tabs[idx].export_cancel.take() {
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    // Drop any capture staged for BackdropReady so it can't restart us.
    app.pending_export = None;
    if app.backdrop_pending == Some(super::backdrop::BackdropKind::Export) {
        app.backdrop_pending = None;
    }
    app.tabs[idx].exporting = false;
    let total = app.tabs[idx].export_total;
    let done = app.tabs[idx].export_done;
    let remaining = total.saturating_sub(done);
    app.tabs[idx].export_failed += remaining;
    app.tabs[idx].export_done = total;
    app.tabs[idx].status = "Export cancelled.".to_string();
    app.export_blur = None;
    Task::none()
}

/// Clears single-use export overlay state once the overlay dismisses.
fn clear_export_overlay(app: &mut App, idx: usize) {
    app.tabs[idx].export_cancel = None;
    // Only the active tab owns the visible blur; a background tab finishing
    // must not clear the overlay of the tab currently being viewed.
    if idx == app.active {
        app.export_blur = None;
    }
}

/// Aborts the export for `tab_id` (tab close): signals the stream loop to
/// stop and drops a staged `pending_export` so `BackdropReady` can't restart it.
pub(crate) fn cancel_export_for_tab(app: &mut App, tab_id: crate::app::tab::TabId) {
    if let Some(pending) = app.pending_export.as_ref()
        && pending.tab_id == tab_id {
            app.pending_export = None;
            app.export_blur = None;
            if app.backdrop_pending == Some(super::backdrop::BackdropKind::Export) {
                app.backdrop_pending = None;
            }
        }
    if let Some(idx) = app.tabs.iter().position(|t| t.id == tab_id) {
        if let Some(flag) = app.tabs[idx].export_cancel.take() {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        app.tabs[idx].exporting = false;
        if idx == app.active {
            app.export_blur = None;
        }
    }
}

#[derive(Clone)]
struct ExportUnit {
    idx: usize,
    image_id: easyscanlate_model::ImageId,
    src_path: String,
    out_path: PathBuf,
    ext: String,
    inpaint: Vec<([f32; 4], RgbaImage)>,
}

fn resolve_out_path(
    folder: &FsPath,
    src_path: &str,
    image_id: easyscanlate_model::ImageId,
    reserved: &std::collections::HashSet<PathBuf>,
) -> (PathBuf, String) {
    let src = FsPath::new(src_path);
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "png".to_string());
    let raw_stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    // Strip leading "<id>_" that mmtl extraction adds (images/<id>_<basename>).
    // Only for export: keep original basename for the output.
    let id_prefix = format!("{}_", image_id.0);
    let stem = if raw_stem.starts_with(&id_prefix) {
        &raw_stem[id_prefix.len()..]
    } else {
        raw_stem
    };
    let stem = if stem.is_empty() { raw_stem } else { stem };
    let mut out_name = format!("{stem}_export.{ext}");
    let mut out_path = folder.join(&out_name);
    // dedup like NewProjectCreate (n), also against this batch's reservations
    if out_path.exists() || reserved.contains(&out_path) {
        let mut n = 1;
        loop {
            out_name = format!("{stem}_export ({n}).{ext}");
            out_path = folder.join(&out_name);
            if !out_path.exists() && !reserved.contains(&out_path) {
                break;
            }
            n += 1;
            if n > 999 {
                break;
            }
        }
    }
    (out_path, ext)
}

fn export_single(unit: &ExportUnit, snapshot: &ExportSnapshot, font: Font) -> Result<String, String> {
    // Load base at native resolution
    let base = match image::open(&unit.src_path) {
        Ok(img) => img.to_rgba8(),
        Err(e) => return Err(format!("{}: open failed: {e}", unit.src_path)),
    };
    let out_rgba = rasterize_page_with_snapshot(base, &unit.inpaint, snapshot, unit.image_id, font);
    // Same encoder settings as before (bit-identical); JPEG avoids a full-image clone.
    match unit.ext.as_str() {
        "jpg" | "jpeg" => {
            let file = std::fs::File::create(&unit.out_path).map_err(|e| e.to_string())?;
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(file, 95);
            // JPEG does not support alpha; conversion to RGB consumes the buffer (no clone).
            let rgb = image::DynamicImage::ImageRgba8(out_rgba).to_rgb8();
            encoder
                .encode(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
                .map_err(|e| e.to_string())?;
            Ok(unit.out_path.display().to_string())
        }
        "png" => out_rgba
            .save_with_format(&unit.out_path, image::ImageFormat::Png)
            .map(|_| unit.out_path.display().to_string())
            .map_err(|e| e.to_string()),
        "bmp" => out_rgba
            .save_with_format(&unit.out_path, image::ImageFormat::Bmp)
            .map(|_| unit.out_path.display().to_string())
            .map_err(|e| e.to_string()),
        "tiff" | "tif" => out_rgba
            .save_with_format(&unit.out_path, image::ImageFormat::Tiff)
            .map(|_| unit.out_path.display().to_string())
            .map_err(|e| e.to_string()),
        "webp" => out_rgba
            .save_with_format(&unit.out_path, image::ImageFormat::WebP)
            .map(|_| unit.out_path.display().to_string())
            .map_err(|e| e.to_string()),
        _ => out_rgba
            .save_with_format(&unit.out_path, image::ImageFormat::Png)
            .map(|_| unit.out_path.display().to_string())
            .map_err(|e| e.to_string()),
    }
    .map_err(|e| format!("{}: {e}", unit.out_path.display()))
}

fn export_chunk_parallel(
    chunk: Vec<ExportUnit>,
    snapshot: &ExportSnapshot,
    font: Font,
) -> super::ExportStreamItem {
    let mut out: super::ExportStreamItem = chunk
        .par_iter()
        .map(|unit| {
            let r = export_single(unit, snapshot, font);
            (unit.idx, r)
        })
        .collect();
    // Deterministic order for error messages / tests.
    out.sort_by_key(|(idx, _)| *idx);
    out
}

pub fn handle_export_finished(app: &mut App, tab_id: crate::app::tab::TabId, result: Result<String, String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    app.tabs[idx].exporting = false;
    clear_export_overlay(app, idx);
    match result {
        Ok(msg) => app.tabs[idx].status = msg,
        Err(e) => app.tabs[idx].status = format!("Export failed: {e}"),
    }
    Task::none()
}

fn finish_export_if_done(app: &mut App, idx: usize) {
    if !app.tabs[idx].exporting || app.tabs[idx].export_done < app.tabs[idx].export_total {
        return;
    }
    app.tabs[idx].exporting = false;
    clear_export_overlay(app, idx);
    let total = app.tabs[idx].export_total;
    let done = app.tabs[idx].export_done;
    let failed = app.tabs[idx].export_failed;
    let saved = done.saturating_sub(failed);
    let folder = app.tabs[idx].export_folder.clone().unwrap_or_default();
    if saved == 0 {
        let detail = if app.tabs[idx].export_errors.is_empty() {
            "No images saved.".to_string()
        } else {
            app.tabs[idx].export_errors.join("; ")
        };
        app.tabs[idx].status = format!("Export failed: {detail}");
    } else {
        // Same final wording as before (bit-identical UX); partial errors stay
        // in `export_errors` and are not appended to keep the message stable.
        let _ = total;
        app.tabs[idx].status = format!("Saved {saved} image(s) to {}", folder.display());
    }
}

pub fn handle_export_stream_run(
    app: &mut App,
    tab_id: crate::app::tab::TabId,
    result: Result<super::ExportStreamItem, String>,
) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    if !app.tabs[idx].exporting {
        return Task::none();
    }
    match result {
        Ok(items) => {
            for (_job_idx, r) in items {
                app.tabs[idx].export_done += 1;
                match r {
                    Ok(_) => {}
                    Err(e) => {
                        app.tabs[idx].export_failed += 1;
                        app.tabs[idx].export_errors.push(e);
                    }
                }
            }
            let done = app.tabs[idx].export_done;
            let total = app.tabs[idx].export_total;
            let failed = app.tabs[idx].export_failed;
            if done < total {
                app.tabs[idx].status = if failed > 0 {
                    format!("Exporting {done} of {total} image(s)... ({failed} failed)")
                } else {
                    format!("Exporting {done} of {total} image(s)...")
                };
            } else {
                finish_export_if_done(app, idx);
            }
        }
        Err(e) => {
            // Whole chunk failed (spawn_blocking cancelled): count the chunk as failed.
            // Remaining chunks won't arrive; finalize with what we have.
            app.tabs[idx].export_errors.push(e);
            app.tabs[idx].exporting = false;
            clear_export_overlay(app, idx);
            let saved = app.tabs[idx].export_done.saturating_sub(app.tabs[idx].export_failed);
            if saved == 0 {
                let detail = if app.tabs[idx].export_errors.is_empty() {
                    "export task failed".to_string()
                } else {
                    app.tabs[idx].export_errors.join("; ")
                };
                app.tabs[idx].status = format!("Export failed: {detail}");
            } else if let Some(folder) = app.tabs[idx].export_folder.clone() {
                app.tabs[idx].status = format!("Saved {saved} image(s) to {}", folder.display());
            }
        }
    }
    Task::none()
}

pub fn handle_export_stream_failed(app: &mut App, tab_id: crate::app::tab::TabId, e: String) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    if !app.tabs[idx].exporting {
        return Task::none();
    }
    app.tabs[idx].exporting = false;
    clear_export_overlay(app, idx);
    // Mark everything not yet done as failed so a dropped stream never hangs at "Exporting...".
    let remaining = app.tabs[idx].export_total.saturating_sub(app.tabs[idx].export_done);
    app.tabs[idx].export_failed += remaining;
    app.tabs[idx].export_done = app.tabs[idx].export_total;
    if !e.is_empty() {
        app.tabs[idx].export_errors.push(e);
    }
    let saved = app.tabs[idx].export_done.saturating_sub(app.tabs[idx].export_failed);
    if saved == 0 {
        let detail = if app.tabs[idx].export_errors.is_empty() {
            "export cancelled".to_string()
        } else {
            app.tabs[idx].export_errors.join("; ")
        };
        app.tabs[idx].status = format!("Export failed: {detail}");
    } else if let Some(folder) = app.tabs[idx].export_folder.clone() {
        app.tabs[idx].status = format!("Saved {saved} image(s) to {}", folder.display());
    }
    Task::none()
}
