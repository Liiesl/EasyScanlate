//! Image inpainting with interchangeable backends: the pure-Rust Telea
//! algorithm from the [`inpaint`] crate (the default: no model, no
//! download), ShiftMap + Poisson (pure Rust, best on textures), Harmonic
//! Laplace diffusion (pure Rust, best on smooth regions), the LaMa ONNX
//! model (`lama-manga.onnx`, fixed 512) and the AOT-GAN ONNX model
//! (`inpainting_aot.onnx`, variable resolution up to 1024, pad=8 — faster +
//! lower memory than LaMa). DirectML execution provider by default on
//! Windows (feature `directml`) for the ONNX backends, with CPU fallback.
//!
//! Module layout: each backend lives in its own module ([`telea`],
//! [`lama`], [`aot`], [`harmonic`], [`shiftmap`]); [`common`] owns the
//! shared crop/mask/compose plumbing; [`poisson`] and [`cg`] are the shared
//! solvers. This file is only the [`Engine`] facade + re-exports.
//!
//! The ONNX backends ([`lama`], [`aot`]) require the crate's `onnx` feature
//! (on by default). With `--no-default-features` the crate compiles without
//! `ort`/`ndarray` so the pure-Rust backends stay fast to test; building a
//! LaMa/AOT engine then returns a descriptive error.
//!
//! [`Engine`] owns the backend chosen at build time. The ONNX backends hold
//! one shared inference session (DirectML by default, CPU fallback) and
//! inpaint image regions one at a time; the Telea backend is stateless and
//! rewrites masked pixels in place. All take the same job: an image path, a
//! selected mask rectangle and the text-box quads inside it; the mask
//! (the rectangle itself, or the quad union when quads are present) is
//! reconstructed from surrounding context — `radius` pixels of context for
//! Telea and LaMa/AOT pixels of real context plus the existing mirror
//! padding (LaMa) or AOT's pad-to-multiple logic — and each box comes back
//! as its own RGBA crop an app layers over the original image without
//! writing anything to disk.

use std::fmt;
#[cfg(feature = "onnx")]
use std::sync::{Arc, Mutex};

use image::RgbaImage;
#[cfg(feature = "onnx")]
use ort::session::Session;
use easyscanlate_model::Quad;
use easyscanlate_settings::InpaintBackend;

pub mod aot;
pub(crate) mod cg;
pub mod common;
pub mod harmonic;
pub mod lama;
pub(crate) mod laplace;
pub mod poisson;
pub mod shiftmap;
pub mod telea;

pub use common::{
    harmonic_inpaint_crop, manual_square_params, shiftmap_inpaint_crop, InpaintPatch, InpaintResult,
};
pub use harmonic::{harmonic_inpaint_rgb, harmonic_inpaint_rgba};
pub use poisson::{poisson_blend, MAX_PIXELS as POISSON_MAX_PIXELS};
pub use shiftmap::{
    shiftmap_inpaint_rgb, MAX_EDGE as SHIFTMAP_MAX_EDGE, N_LABELS as SHIFTMAP_N_LABELS,
    PATCH as SHIFTMAP_PATCH,
};
pub use telea::telea_inpaint_crop;
// LaMa/AOT model entry points need ORT; the geometry consts stay available
// without `onnx` so callers can keep single import paths.
#[cfg(feature = "onnx")]
pub use aot::aot_inpaint_crop;
pub use aot::{AOT_MAX_SIZE, AOT_PAD};
#[cfg(feature = "onnx")]
pub use lama::inpaint_crop;
pub use lama::MODEL_EDGE;

/// Cloneable handle to the shared inpainting engine: either a stateless
/// pure-Rust backend or an ONNX session (one inference at a time, serialized
/// through the inner mutex).
#[derive(Clone)]
pub struct Engine {
    backend: InpaintBackend,
    /// Telea's interpolation radius in pixels; ignored by the ONNX backends.
    radius: i32,
    /// The shared ONNX session; `None` for the pure-Rust backends.
    /// Absent entirely without the `onnx` feature.
    #[cfg(feature = "onnx")]
    session: Option<Arc<Mutex<Session>>>,
}

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Engine")
            .field("backend", &self.backend)
            .field("radius", &self.radius)
            .finish()
    }
}

impl Engine {
    /// Builds the engine for `backend`. Telea/ShiftMap/Harmonic are instant
    /// and stateless; the ONNX backends load their model with DirectML on
    /// Windows (feature `directml`) and fallback to CPU on failure.
    /// Without the crate's `onnx` feature, LaMa/AOT return an error.
    pub fn build(backend: InpaintBackend, radius: i32) -> Result<Self, String> {
        #[cfg(feature = "onnx")]
        let session = match backend {
            InpaintBackend::Telea
            | InpaintBackend::ShiftMap
            | InpaintBackend::Harmonic => None,
            InpaintBackend::Lama => Some(Arc::new(Mutex::new(lama::build_session()?))),
            InpaintBackend::Aot => Some(Arc::new(Mutex::new(aot::build_session()?))),
        };
        #[cfg(not(feature = "onnx"))]
        if matches!(backend, InpaintBackend::Lama | InpaintBackend::Aot) {
            return Err(format!(
                "{backend} backend requires the `onnx` feature (rebuild without `--no-default-features`)"
            ));
        }
        Ok(Self {
            backend,
            radius: radius.max(1),
            #[cfg(feature = "onnx")]
            session,
        })
    }

    /// The backend this engine was built for; the app rebuilds the engine
    /// when the configured backend or radius changes.
    pub fn backend(&self) -> InpaintBackend {
        self.backend
    }

    /// The Telea interpolation radius this engine was built with.
    pub fn radius(&self) -> i32 {
        self.radius
    }

    /// Decodes `path` and inpaints the selected mask `rect`, masking out the
    /// given quads (or the whole `rect` when there are none) and sampling
    /// from surrounding context. The surrounding context is the Telea
    /// `radius` (`settings::inpaint_radius`) for the Telea backend and
    /// LaMa/AOT pixels of real context plus the existing padding
    /// for the ONNX backends.
    /// The original file is never modified; each returned patch is the RGBA
    /// crop of one mask quad (`[x, y, w, h]` in image pixels), with the
    /// text reconstructed by the engine's backend. For an empty quad list
    /// the single returned patch covers the (clamped) `rect` itself.
    /// The patch image has `alpha=0` outside the actual rotated quad so
    /// only the quad interior is composited. The third element is the
    /// corresponding quad (`Some` when input quads non-empty, `None` for
    /// empty-list whole-rect case).
    pub fn run_blocking(
        &self,
        path: &str,
        rect: [f32; 4],
        quads: &[Quad],
    ) -> InpaintResult {
        let image = image::ImageReader::open(path)
            .map_err(|e| format!("Failed to open {path}: {e}"))?
            .with_guessed_format()
            .map_err(|e| format!("Failed to decode {path}: {e}"))?
            .decode()
            .map_err(|e| format!("Failed to decode {path}: {e}"))?
            .into_rgba8();
        match self.backend {
            InpaintBackend::Telea => telea_inpaint_crop(&image, rect, quads, self.radius),
            InpaintBackend::ShiftMap => shiftmap_inpaint_crop(&image, rect, quads),
            InpaintBackend::Harmonic => harmonic_inpaint_crop(&image, rect, quads, self.radius),
            #[cfg(feature = "onnx")]
            InpaintBackend::Lama => {
                let mut session = self
                    .session
                    .as_ref()
                    .ok_or("LaMa engine has no session")?
                    .lock()
                    .map_err(|e| format!("Inpaint engine lock poisoned: {e}"))?;
                inpaint_crop(&mut session, &image, rect, quads)
            }
            #[cfg(feature = "onnx")]
            InpaintBackend::Aot => {
                let mut session = self
                    .session
                    .as_ref()
                    .ok_or("AOT engine has no session")?
                    .lock()
                    .map_err(|e| format!("Inpaint engine lock poisoned: {e}"))?;
                aot_inpaint_crop(&mut session, &image, rect, quads)
            }
            #[cfg(not(feature = "onnx"))]
            InpaintBackend::Lama | InpaintBackend::Aot => Err(format!(
                "{} backend requires the `onnx` feature (rebuild without `--no-default-features`)",
                self.backend
            )),
        }
    }

    /// Like [`Self::run_blocking`] but for an already-decoded [`RgbaImage`].
    /// Used for stitched canvases that span two pages (manual in-between
    /// inpaint). The `rect` and `quads` are in the passed image's pixel
    /// space. For the stitched path every backend follows Lama's 512-edge
    /// handling (window / resize) is inside `inpaint_crop` / `aot_inpaint_crop`;
    /// Telea simply expands by `radius`.
    pub fn run_on_image(
        &self,
        image: &RgbaImage,
        rect: [f32; 4],
        quads: &[Quad],
    ) -> InpaintResult {
        match self.backend {
            InpaintBackend::Telea => telea_inpaint_crop(image, rect, quads, self.radius),
            InpaintBackend::ShiftMap => shiftmap_inpaint_crop(image, rect, quads),
            InpaintBackend::Harmonic => harmonic_inpaint_crop(image, rect, quads, self.radius),
            #[cfg(feature = "onnx")]
            InpaintBackend::Lama => {
                let mut session = self
                    .session
                    .as_ref()
                    .ok_or("LaMa engine has no session")?
                    .lock()
                    .map_err(|e| format!("Inpaint engine lock poisoned: {e}"))?;
                inpaint_crop(&mut session, image, rect, quads)
            }
            #[cfg(feature = "onnx")]
            InpaintBackend::Aot => {
                let mut session = self
                    .session
                    .as_ref()
                    .ok_or("AOT engine has no session")?
                    .lock()
                    .map_err(|e| format!("Inpaint engine lock poisoned: {e}"))?;
                aot_inpaint_crop(&mut session, image, rect, quads)
            }
            #[cfg(not(feature = "onnx"))]
            InpaintBackend::Lama | InpaintBackend::Aot => Err(format!(
                "{} backend requires the `onnx` feature (rebuild without `--no-default-features`)",
                self.backend
            )),
        }
    }
}
