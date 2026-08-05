//! Granular change events for the live `Project` DB.
//! Mutating methods on `Project` return one of these synchronously
//! so the `src/app` message hub (`Message`) can react without a coarse
//! `DataChanged` broadcast.

use crate::{EntryId, ImageId, InpaintId, ProfileId, Quad};

#[derive(Debug, Clone, PartialEq)]
pub enum ModelEvent {
    ImageAdded {
        image_id: ImageId,
    },
    EntriesAdded {
        image_id: ImageId,
        ids: Vec<EntryId>,
    },
    EntryDeleted {
        id: EntryId,
    },
    EntryRestored {
        id: EntryId,
    },
    EntryTextUpdated {
        id: EntryId,
        profile: ProfileId,
    },
    EntryMoved {
        id: EntryId,
        quad: Quad,
    },
    EntryStyleUpdated {
        id: EntryId,
    },
    /// Only the entries of `image_id` were reordered (Y→X via view_quad).
    EntriesReordered {
        image_id: ImageId,
    },
    ProfileCreated {
        id: ProfileId,
        name: String,
    },
    ProfileRemoved {
        id: ProfileId,
    },
    ProfileSelected {
        id: ProfileId,
    },
    ProfileRenamed {
        id: ProfileId,
        name: String,
    },
    InpaintAdded {
        id: InpaintId,
        image_id: ImageId,
        bounds: [f32; 4],
    },
    InpaintRemoved {
        id: InpaintId,
    },
    NoteUpdated {
        entry: EntryId,
    },
}
