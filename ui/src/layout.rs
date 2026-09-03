/// Widget id of the floating inline editor shown over a double-clicked entry.
pub const EDIT_INPUT_ID: &str = "overlay-editor";

/// Widget id of the multi-line editor shown in a results-list row while the
/// entry is edited from the panel.
pub const PANEL_EDIT_INPUT_ID: &str = "panel-editor";

pub const IMAGE_FILTERS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp", "tiff", "avif"];

/// The pane the side panel occupies at launch: ~74% of the default window
/// width (about 1036px of the 1400px window), leaving the main area a third
/// of its previous ~1120px default.
pub const MAIN_AREA_DEFAULT_RATIO: f32 = 0.26;

/// Default share of the styling panel vs the results panel inside the side pane.
pub const STYLING_DEFAULT_RATIO: f32 = 0.36;

/// Minimum width of the styling column (left) in the side panel — fixed pixels, not font-scaled.
pub const STYLING_MIN_WIDTH: f32 = 260.0;

/// Minimum width of the main area — fixed pixels, different from panel.
pub const MAIN_AREA_MIN_WIDTH: f32 = 160.0;

/// Default share of the styling inspector vs the inpaint/layers list inside the styling column.
/// ~70% top (taller), 30% bottom (shorter, not dramatic) – vertically stacked, resizable.
pub const STYLING_TOP_RATIO: f32 = 0.70;

/// Transparent gap shown between every top-level component (toolbar / main area / action / styling / results).
pub const GAP: f32 = 12.0;

/// Corner radius of the floating panel cards.
pub const CARD_RADIUS: f32 = 12.0;

/// Padding around the whole app window — shows the aurora as an outer frame.
pub const OUTER_PADDING: f32 = 10.0;

/// The two panes of the app window: the page viewer and the side panel.
#[derive(Debug, Clone, Copy)]
pub enum PaneKind {
    MainArea,
    Panel,
}

/// The two panes inside the side panel: styling on the left, results/translation on the right.
#[derive(Debug, Clone, Copy)]
pub enum SidePaneKind {
    Styling,
    Results,
}

/// The two stacked panes inside the styling column: inspector on top (taller), inpaint/layers list at bottom.
#[derive(Debug, Clone, Copy)]
pub enum StylingPaneKind {
    Inspector,
    Layers,
}
