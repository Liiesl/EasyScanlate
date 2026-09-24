/// Widget id of the floating inline editor shown over a double-clicked entry.
pub const EDIT_INPUT_ID: &str = "overlay-editor";

/// Widget id of the multi-line editor shown in a results-list row while the
/// entry is edited from the panel.
pub const PANEL_EDIT_INPUT_ID: &str = "panel-editor";

pub const IMAGE_FILTERS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp", "tiff", "avif"];

/// Share of the main area vs the styling panel inside the right side
/// (the left Translation/Inpaint column lives outside this split, left of
/// the toolbar). ~55% main, ~45% styling at launch.
pub const MAIN_AREA_DEFAULT_RATIO: f32 = 0.55;

/// Minimum width of either pane in the right-side Main/Styling split —
/// shared by `pane_grid::min_size`, so it floors both panes equally.
pub const MAIN_AREA_MIN_WIDTH: f32 = 160.0;

/// Default share of the translation list vs the inpaint/layers list inside
/// the left column. ~70% top (taller), 30% bottom (shorter, not dramatic) –
/// vertically stacked, resizable.
pub const RESULTS_TOP_RATIO: f32 = 0.70;

/// Transparent gap shown between every top-level component (toolbar / main area / action / styling / results).
pub const GAP: f32 = 12.0;

/// Corner radius of the floating panel cards.
pub const CARD_RADIUS: f32 = 12.0;

/// Padding around the whole app window — shows the aurora as an outer frame.
pub const OUTER_PADDING: f32 = 10.0;

/// Share of the left Translation/Inpaint column vs everything right of it
/// (toolbar + main canvas + styling) at launch. Narrower than the old
/// results column so the canvas keeps room.
pub const EDITOR_LEFT_DEFAULT_RATIO: f32 = 0.30;

/// The two panes of the editor's outer split: the left
/// Translation/Inpaint column vs everything right of the toolbar.
#[derive(Debug, Clone, Copy)]
pub enum EditorPaneKind {
    Left,
    Right,
}

/// The two panes of the right side (right of the toolbar): the page viewer
/// and the styling inspector.
#[derive(Debug, Clone, Copy)]
pub enum PaneKind {
    MainArea,
    Styling,
}

/// The two stacked panes inside the left column (left of the toolbar):
/// translation/results on top (taller), inpaint/layers list at bottom.
#[derive(Debug, Clone, Copy)]
pub enum ResultsPaneKind {
    Translation,
    Layers,
}
