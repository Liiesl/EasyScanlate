//! Pure data model: the app's single source of truth / live DB.
//!
//! Rules enforced here:
//! - [`Project`] is the live DB for **project data only**: `images` (path/dims),
//!   `ocr` (append-only `OcrResult`), `profiles` (chapter-wide delta layer),
//!   `styles` (per-entry, shared across profiles), `view_quads`, `extras`
//!   (notes, inpaint patches, shapes). Nothing else.
//! - Configuration / appearance / behaviour flags (`aurora_*`, `auto_style_detect`,
//!   `auto_inpaint`, `auto_sfx_filter`, `auto_inpaint_model`, `inpaint_backend`,
//!   `inpaint_radius`, `ocr_*`, `ui_font_size`, `last_provider`, `connections`
//!   / `api_key`, `hidden_models`, `free_models_only`, `recent_projects`,
//!   `style_presets` templates) live in `scanlateit-settings`, not here.
//!   Preset *templates* (`StylePresets: Vec<Option<EntryStyle>>`) reuse the
//!   `EntryStyle` type but are user-level config; per-entry styles remain here.
//! - [`OcrResult`] is append-only; deletion is a soft-delete flag.
//! - [`Profiles`] are a freely editable delta layer on top of OCR; one profile
//!   is selected at a time and the whole UI reads through it.
//! - Entry styles and `view_quads` are per-OCR-result dicts shared by every
//!   profile (see [`Project`]), not per-profile deltas.
//! - [`Extras`] (notes, inpainting, geometries) survives across profiles.
//! - Data is entry-centric: `OcrEntry { image_id: ImageId }`, `EntryId` globally
//!   unique. `Project::visible_entries()` / `visible_for(ImageId)` hide `deleted`;
//!   `all_*` / `entry_including_deleted` are explicit escape hatches.
//! - Ordering / filtering / profile resolution is centralized in `Project`
//!   (`visible_*`, `display_text*`, `reorder_entries_for_image`, `resolved_text_for`).
//!   Callers must not manually filter `deleted` or re-derive Y→X ordering.
//! - All mutating methods have a `*_with_event` variant returning a synchronous
//!   `ModelEvent`; the hub `src/app.rs: Message::Model(ModelEvent)` consumes it
//!   inline via `handle_model_event` (no channel). Raw mutators are `pub(crate)`
//!   or batch-only; app code must go through `*_with_event`.
//! - `InpaintPatch` is first-class with stable `InpaintId` (model holds
//!   `bounds`+`image_id`; pixels/`Handle` live outside in `ui::LoadedImage` /
//!   `mmtl` temp dir).
//!
//! This layer depends only on `std+serde`: no iced, no confy, no OCR engine, no I/O.

pub mod entry;
pub mod event;
pub mod extras;
pub mod ocr_result;
pub mod profile;
pub mod project;
pub mod style;

pub use entry::{EntryId, EntrySource, ImageId, ImageMeta, NewEntry, OcrEntry, Quad};
pub use event::ModelEvent;
pub use extras::{Extras, InpaintId, InpaintPatch, Shape, ShapeKind};
pub use ocr_result::OcrResult;
pub use profile::{EntryDelta, Profile, ProfileId, Profiles};
pub use project::Project;
pub use style::{
    EntryStyle, TextAlign, TextGradientDir, ANIME_ACE_FAMILY, AUGIE_FAMILY, BUNDLED_FONTS,
    DEFAULT_FONT_FAMILY,
};