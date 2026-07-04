use std::time::Duration;

use iced::Color;

pub const SCROLL_LINE_HEIGHT: f32 = 180.0;
pub const SCROLLBAR_WIDTH: f32 = 8.0;
pub const SCROLLBAR_MARGIN: f32 = 2.0;
pub const MIN_THUMB_HEIGHT: f32 = 20.0;

/// Maximum gap between two presses on the same entry to count as a double-click.
pub const DOUBLE_CLICK_DELAY: Duration = Duration::from_millis(400);

/// Cursor movement (viewport pixels) needed before a press on an entry turns into a drag.
pub const DRAG_THRESHOLD: f32 = 3.0;

/// Side of a resize handle square, in viewport pixels.
pub const HANDLE_SIZE: f32 = 8.0;
/// Smallest box edge allowed while resizing, in viewport pixels.
pub const MIN_BOX_EDGE: f32 = 6.0;

/// Smallest inpainting range edge, in image pixels.
pub const MIN_INPAINT_EDGE: f32 = 4.0;

/// Selection toolbar geometry, in viewport/tile pixels.
pub const TOOLBAR_HEIGHT: f32 = 22.0;
pub const TOOLBAR_GAP: f32 = 5.0;
pub const TOOLBAR_BTN_PAD: f32 = 10.0;
pub const TOOLBAR_BG: Color = Color::from_rgba8(28, 30, 38, 0.96);
pub const TOOLBAR_HOVER_BG: Color = Color::from_rgba8(58, 62, 76, 1.0);
pub const TOOLBAR_FG: Color = Color::from_rgba8(215, 220, 235, 1.0);
pub const HANDLE_FILL: Color = Color::WHITE;
pub const HANDLE_BORDER: Color = Color::from_rgba8(92, 190, 255, 1.0);

/// Geometry of the quick-action overlay buttons pinned to the viewer's bottom-left corner.
pub const OVERLAY_BTN_WIDTH: f32 = 56.0;
pub const OVERLAY_BTN_HEIGHT: f32 = 24.0;
pub const OVERLAY_BTN_GAP: f32 = 6.0;
pub const OVERLAY_BTN_MARGIN: f32 = 10.0;

/// Length of the stem connecting the rotation knob to the box.
pub const ROTATE_STEM: f32 = 16.0;

pub const PLACEHOLDER_BG: Color = Color::from_rgba8(45, 47, 60, 1.0);
pub const PLACEHOLDER_FG: Color = Color::from_rgba8(140, 145, 160, 1.0);
pub const FAILED_BG: Color = Color::from_rgba8(70, 40, 45, 1.0);
pub const FAILED_FG: Color = Color::from_rgba8(200, 120, 120, 1.0);
pub const SCROLLBAR_TRACK: Color = Color::from_rgba8(255, 255, 255, 0.07);
pub const SCROLLBAR_THUMB: Color = Color::from_rgba8(255, 255, 255, 0.35);

/// Inpainting range marquee colors.
pub const INPAINT_FILL: Color = Color::from_rgba8(92, 190, 255, 0.16);
pub const INPAINT_STROKE: Color = Color::from_rgba8(92, 190, 255, 1.0);
