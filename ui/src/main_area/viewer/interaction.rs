use iced::advanced::mouse;
use iced::Point;

use scanlateit_model::{EntryId, Quad};

use crate::event::{InpaintToolbarAction, ToolbarAction};
use lucide_icons::Icon;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayButton {
    GoTop,
    Save,
    GoBottom,
}

impl OverlayButton {
    pub fn column() -> [(OverlayButton, Icon); 3] {
        [
            (OverlayButton::GoTop, Icon::ArrowUp),
            (OverlayButton::Save, Icon::Download),
            (OverlayButton::GoBottom, Icon::ArrowDown),
        ]
    }

    pub fn icon(self) -> Icon {
        match self {
            OverlayButton::GoTop => Icon::ArrowUp,
            OverlayButton::Save => Icon::Download,
            OverlayButton::GoBottom => Icon::ArrowDown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Interaction {
    None,
    TouchScrolling { origin: Point },
    ScrollerGrabbed { grab_offset: f32 },
    DragPending {
        index: usize,
        id: EntryId,
        offset: [f32; 2],
        quad: Quad,
        press: Point,
    },
    Dragging {
        index: usize,
        id: EntryId,
        offset: [f32; 2],
        quad: Quad,
    },
    ResizePending {
        index: usize,
        id: EntryId,
        handle: ResizeHandle,
        quad: Quad,
        press: Point,
    },
    Resizing {
        index: usize,
        id: EntryId,
        handle: ResizeHandle,
        quad: Quad,
    },
    DistortPending {
        index: usize,
        id: EntryId,
        corner: usize,
        quad: Quad,
        press: Point,
    },
    Distorting {
        index: usize,
        id: EntryId,
        corner: usize,
        quad: Quad,
    },
    RotatePending {
        index: usize,
        id: EntryId,
        quad: Quad,
        center_img: [f32; 2],
        center_view: Point,
        press: Point,
    },
    Rotating {
        index: usize,
        id: EntryId,
        quad: Quad,
        center_img: [f32; 2],
        center_view: Point,
        press: Point,
    },
    ToolbarPressed {
        index: usize,
        id: EntryId,
        action: ToolbarAction,
    },
    InpaintToolbarPressed {
        index: usize,
        patch: usize,
        action: InpaintToolbarAction,
    },
    OverlayButtonPressed {
        button: OverlayButton,
    },
    InpaintSelecting {
        index: usize,
        start: Point,
        current: Point,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeHandle {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

impl ResizeHandle {
    pub const NW: Self = Self { left: true, right: false, top: true, bottom: false };
    pub const N: Self = Self { left: false, right: false, top: true, bottom: false };
    pub const NE: Self = Self { left: false, right: true, top: true, bottom: false };
    pub const E: Self = Self { left: false, right: true, top: false, bottom: false };
    pub const SE: Self = Self { left: false, right: true, top: false, bottom: true };
    pub const S: Self = Self { left: false, right: false, top: false, bottom: true };
    pub const SW: Self = Self { left: true, right: false, top: false, bottom: true };
    pub const W: Self = Self { left: true, right: false, top: false, bottom: false };

    pub fn cursor(self) -> mouse::Interaction {
        match self {
            Self { left: true, right: false, top: true, bottom: false }
            | Self { left: false, right: true, top: false, bottom: true } => {
                mouse::Interaction::ResizingDiagonallyDown
            }
            Self { left: false, right: true, top: true, bottom: false }
            | Self { left: true, right: false, top: false, bottom: true } => {
                mouse::Interaction::ResizingDiagonallyUp
            }
            Self { top: true, .. } | Self { bottom: true, .. } => {
                mouse::Interaction::ResizingVertically
            }
            _ => mouse::Interaction::ResizingHorizontally,
        }
    }

    pub fn corner(self) -> Option<usize> {
        match self {
            Self::NW => Some(0),
            Self::NE => Some(1),
            Self::SE => Some(2),
            Self::SW => Some(3),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopDecorHit {
    Rotate,
    Revert,
}

pub struct TopDecor {
    pub anchor: Point,
    pub stem_from: Point,
    pub revert: iced::Rectangle,
}
