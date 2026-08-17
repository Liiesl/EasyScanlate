use std::cell::RefCell;
use std::path::{Path as FsPath, PathBuf};
use std::rc::Rc;

use iced::advanced::graphics::geometry::{self, Fill, Stroke, Text};
use iced::advanced::graphics::geometry::Path as GeomPath;
use iced::{Font, Point, Rectangle, Size, Vector, Radians};
use image::RgbaImage;
use tiny_skia::{FillRule, Mask, Paint, Pixmap, PremultipliedColorU8, Rect, Shader, Transform};

use scanlateit_model::Project;
use scanlateit_ui::main_area::overlay::{self, OverlayEntry};

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
}

impl ExportFrame {
    fn new(pixmap: Pixmap) -> Self {
        Self {
            pixmap: Rc::new(RefCell::new(pixmap)),
            transform: Transform::identity(),
            stack: Vec::new(),
            clip: None,
            clip_stack: Vec::new(),
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
            (*self_ptr).stroke(&path, stroke.clone());
        });
    }

    fn draw_image(&mut self, _bounds: Rectangle, _image: impl Into<geometry::Image>) {
        // overlays don't use draw_image; inpaint/base already composited via image crate
    }
    fn draw_svg(&mut self, _bounds: Rectangle, _svg: impl Into<geometry::Svg>) {}

    fn into_geometry(self) -> Self::Geometry {}
}

// ---------------------------------------------------------------------------
// Helpers: Pixmap <-> RgbaImage
// ---------------------------------------------------------------------------
fn rgba_to_pixmap(img: &RgbaImage) -> Pixmap {
    let (w, h) = img.dimensions();
    let mut pix = Pixmap::new(w, h).unwrap_or_else(|| Pixmap::new(1, 1).unwrap());
    // tiny-skia expects premultiplied; image crate stores straight
    for (i, px) in img.pixels().enumerate() {
        let [r, g, b, a] = px.0;
        // premultiply
        let pa = PremultipliedColorU8::from_rgba(r, g, b, a).unwrap_or(PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap());
        pix.pixels_mut()[i] = pa;
    }
    pix
}

fn pixmap_to_rgba(pix: Pixmap) -> RgbaImage {
    let w = pix.width();
    let h = pix.height();
    // unpremultiply to straight for image crate png/jpeg
    let data: Vec<u8> = pix
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
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

fn rasterize_page(
    base: RgbaImage,
    inpaint_raw: &[( [f32;4], RgbaImage )],
    project: &Project,
    image_id: scanlateit_model::ImageId,
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
    let images = project.images();
    let mut global_offsets: Vec<f32> = Vec::with_capacity(images.len());
    let mut cur = 0.0f32;
    for m in images {
        global_offsets.push(cur);
        cur += m.height;
    }
    let page_idx = images.iter().position(|m| m.id == image_id).unwrap_or(0);
    let page_g0 = global_offsets.get(page_idx).copied().unwrap_or(0.0);
    let page_g1 = page_g0 + h as f32;
    // owner -> idx map for O(1)
    let mut owner_to_idx: std::collections::HashMap<scanlateit_model::ImageId, usize> = std::collections::HashMap::new();
    for (i, m) in images.iter().enumerate() {
        owner_to_idx.insert(m.id, i);
    }

    let mut texts: Vec<String> = Vec::new();
    let mut metas: Vec<(scanlateit_model::EntryId, scanlateit_model::Quad, scanlateit_model::EntryStyle)> = Vec::new();

    for e in project.visible_entries() {
        let orig_quad = project.view_quad(e);
        let [vx0, vy0, vx1, vy1] = orig_quad.bounds();
        let owner_idx = match owner_to_idx.get(&e.image_id) {
            Some(v) => *v,
            None => continue,
        };
        let owner_g0 = global_offsets.get(owner_idx).copied().unwrap_or(0.0);
        let owner_meta = project.image(e.image_id);
        let owner_w = owner_meta.map(|m| m.width).unwrap_or(w as f32);
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
        texts.push(project.display_text(e).to_string());
        metas.push((e.id, q, project.entry_style(e.id)));
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
    if app.project.images().is_empty() || app.images.is_empty() {
        app.status = "Nothing to export.".to_string();
        return Task::none();
    }
    // default folder = mmtl parent or first image parent
    let default_dir = app
        .mmtl_path
        .as_ref()
        .and_then(|p| p.parent().map(|par| par.to_path_buf()))
        .or_else(|| {
            app.project
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
        Message::ExportFolderPicked,
    )
}

pub fn handle_export_picked(app: &mut App, folder: Option<String>) -> Task<Message> {
    let Some(folder_str) = folder else {
        app.status = "Export cancelled.".to_string();
        return Task::none();
    };
    let folder_path = PathBuf::from(&folder_str);
    if !folder_path.exists() {
        if let Err(e) = std::fs::create_dir_all(&folder_path) {
            app.status = format!("Export failed: cannot create folder: {e}");
            return Task::none();
        }
    }

    // Snapshot project + inpaint raw for blocking thread
    let project = app.project.clone();
    let font = app.font.unwrap_or(Font::DEFAULT);

    // Extract inpaint raw per image (bounds + RgbaImage) to avoid Handle Send issues
    let mut inpaint_per_image: Vec<Vec<([f32; 4], RgbaImage)>> = Vec::new();
    for loaded in &app.images {
        let mut v = Vec::new();
        for layer in &loaded.inpaint {
            if let Some(rgba) = handle_to_rgba(&layer.handle) {
                v.push((layer.bounds, rgba));
            }
        }
        inpaint_per_image.push(v);
    }

    // Also need original paths and ids
    let metas: Vec<(scanlateit_model::ImageId, String)> = project
        .images()
        .iter()
        .map(|m| (m.id, m.path.clone()))
        .collect();

    app.status = format!("Exporting {} image(s)...", metas.len());

    let folder_clone = folder_path.clone();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || export_blocking(project, metas, inpaint_per_image, folder_clone, font))
                .await
                .unwrap_or_else(|e| Err(format!("export task failed: {e}")))
        },
        Message::ExportFinished,
    )
}

fn export_blocking(
    project: Project,
    metas: Vec<(scanlateit_model::ImageId, String)>,
    inpaint_per_image: Vec<Vec<([f32; 4], RgbaImage)>>,
    folder: PathBuf,
    font: Font,
) -> Result<String, String> {
    let mut saved = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for (idx, (image_id, src_path)) in metas.iter().enumerate() {
        let inpaint_raw = inpaint_per_image.get(idx).map(|v| v.as_slice()).unwrap_or(&[]);

        // Load base at native resolution
        let base = match image::open(src_path) {
            Ok(img) => img.to_rgba8(),
            Err(e) => {
                errors.push(format!("{}: open failed: {e}", src_path));
                continue;
            }
        };

        let out_rgba = rasterize_page(base, inpaint_raw, &project, *image_id, font);

        // Determine output extension & path, keep original format
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
        // dedup like NewProjectCreate (n)
        if out_path.exists() {
            let mut n = 1;
            loop {
                out_name = format!("{stem}_export ({n}).{ext}");
                out_path = folder.join(&out_name);
                if !out_path.exists() {
                    break;
                }
                n += 1;
                if n > 999 {
                    break;
                }
            }
        }

        let res: Result<(), String> = (|| {
            // map original quality if possible: for jpeg use 95 (high quality, keep original format)
            match ext.as_str() {
                "jpg" | "jpeg" => {
                    let file = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
                    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(file, 95);
                    // JPEG does not support alpha; composite onto white via conversion to RGB
                    let rgb = image::DynamicImage::ImageRgba8(out_rgba.clone()).to_rgb8();
                    encoder.encode(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
                    Ok(())
                }
                "png" => {
                    out_rgba.save_with_format(&out_path, image::ImageFormat::Png).map_err(|e| e.to_string())?;
                    Ok(())
                }
                "bmp" => {
                    out_rgba.save_with_format(&out_path, image::ImageFormat::Bmp).map_err(|e| e.to_string())?;
                    Ok(())
                }
                "tiff" | "tif" => {
                    out_rgba.save_with_format(&out_path, image::ImageFormat::Tiff).map_err(|e| e.to_string())?;
                    Ok(())
                }
                "webp" => {
                    out_rgba.save_with_format(&out_path, image::ImageFormat::WebP).map_err(|e| e.to_string())?;
                    Ok(())
                }
                _ => {
                    // fallback png but keep requested ext? Save as png with that ext will be confusing, so just png
                    out_rgba.save_with_format(&out_path, image::ImageFormat::Png).map_err(|e| e.to_string())?;
                    Ok(())
                }
            }
        })();

        match res {
            Ok(_) => saved += 1,
            Err(e) => errors.push(format!("{}: {e}", out_path.display())),
        }
    }

    if saved == 0 && !errors.is_empty() {
        return Err(errors.join("; "));
    }

    let msg = format!("Saved {saved} image(s) to {}", folder.display());
    if saved == 0 {
        Err("No images saved.".to_string())
    } else {
        Ok(msg)
    }
}

pub fn handle_export_finished(app: &mut App, result: Result<String, String>) -> Task<Message> {
    match result {
        Ok(msg) => app.status = msg,
        Err(e) => app.status = format!("Export failed: {e}"),
    }
    Task::none()
}
