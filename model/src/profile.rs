//! Translation variants of a project. Profiles are a pure delta layer on top
//! of the immutable OCR result: one profile is selected, the rest read through
//! it. All mutations go via `Project::*_with_event`.

use std::collections::HashMap;

use super::EntryId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProfileId(pub u64);

/// Per-entry delta: everything a profile changes about an OCR entry.
#[derive(Debug, Clone)]
pub struct EntryDelta {
    /// Translated text; `None` falls back to the entry's OCR text.
    pub translation: Option<String>,
}

/// One translation variant of the document model.
#[derive(Debug, Clone)]
pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    deltas: HashMap<EntryId, EntryDelta>,
}

impl Profile {
    fn new(id: ProfileId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            deltas: HashMap::new(),
        }
    }

    pub fn translation_of(&self, entry_id: EntryId) -> Option<&str> {
        self.deltas.get(&entry_id).and_then(|d| d.translation.as_deref())
    }

    /// Set (or clear) the translated text for an entry.
    pub fn set_translation(&mut self, entry_id: EntryId, translation: Option<String>) {
        let delta = self.deltas.entry(entry_id).or_insert(EntryDelta {
            translation: None,
        });
        delta.translation = translation;
        self.prune(entry_id);
    }

    /// Drop the delta for an entry, falling back to OCR text.
    pub fn reset(&mut self, entry_id: EntryId) {
        self.deltas.remove(&entry_id);
    }

    /// The delta for an entry, if any.
    pub fn delta(&self, entry_id: EntryId) -> Option<&EntryDelta> {
        self.deltas.get(&entry_id)
    }

    /// All deltas (for persistence).
    pub fn deltas(&self) -> &std::collections::HashMap<EntryId, EntryDelta> {
        &self.deltas
    }

    /// Reconstruct from raw parts (for persistence).
    pub fn from_raw(id: ProfileId, name: String, deltas: std::collections::HashMap<EntryId, EntryDelta>) -> Self {
        Self { id, name, deltas }
    }

    fn prune(&mut self, entry_id: EntryId) {
        let empty = self
            .deltas
            .get(&entry_id)
            .is_some_and(|d| d.translation.is_none());
        if empty {
            self.deltas.remove(&entry_id);
        }
    }
}

/// All profiles plus the one currently selected.
#[derive(Debug, Clone)]
pub struct Profiles {
    profiles: Vec<Profile>,
    selected: ProfileId,
    next_id: u64,
}

impl Default for Profiles {
    fn default() -> Self {
        let mut profiles = Self {
            profiles: Vec::new(),
            selected: ProfileId(0),
            next_id: 1,
        };
        profiles.profiles.push(Profile::new(ProfileId(0), "Default"));
        profiles
    }
}

impl Profiles {
    pub fn add(&mut self, name: impl Into<String>) -> ProfileId {
        let id = ProfileId(self.next_id);
        self.next_id += 1;
        self.profiles.push(Profile::new(id, name));
        id
    }

    /// Remove a profile. The selected profile and the last remaining profile
    /// cannot be removed.
    pub fn remove(&mut self, id: ProfileId) -> bool {
        if id == self.selected || self.profiles.len() <= 1 {
            return false;
        }
        let before = self.profiles.len();
        self.profiles.retain(|p| p.id != id);
        self.profiles.len() != before
    }

    pub fn select(&mut self, id: ProfileId) -> bool {
        if self.profiles.iter().any(|p| p.id == id) {
            self.selected = id;
            true
        } else {
            false
        }
    }

    /// The id of the profile with the given name, if any.
    pub fn find_by_name(&self, name: &str) -> Option<ProfileId> {
        self.profiles.iter().find(|p| p.name == name).map(|p| p.id)
    }

    /// The id of the profile created with the project ("Default"), the only
    /// profile whose text is never edited in place: edits made while it is
    /// selected are forked into a fresh profile instead, keeping the source
    /// of truth (the OCR result) immutable.
    pub fn original_id(&self) -> ProfileId {
        self.profiles
            .first()
            .map(|p| p.id)
            .unwrap_or(ProfileId(0))
    }

    /// The next unused auto-forked profile name: `Profile 1`, `Profile 2`,
    /// ... continuing past the highest existing number.
    pub fn next_available_name(&self) -> String {
        let highest = self
            .profiles
            .iter()
            .filter_map(|p| p.name.strip_prefix("Profile ").and_then(|n| n.parse::<u64>().ok()))
            .max()
            .unwrap_or(0);
        format!("Profile {}", highest + 1)
    }

    pub fn selected_id(&self) -> ProfileId {
        self.selected
    }

    pub fn selected(&self) -> &Profile {
        self.profiles
            .iter()
            .find(|p| p.id == self.selected)
            .expect("selected profile always exists")
    }

    pub fn selected_mut(&mut self) -> &mut Profile {
        self.profiles
            .iter_mut()
            .find(|p| p.id == self.selected)
            .expect("selected profile always exists")
    }

    pub fn iter(&self) -> impl Iterator<Item = &Profile> {
        self.profiles.iter()
    }

    /// Forks a fresh profile off the original ("Default") profile and selects
    /// it, for inline edits that must not touch the OCR source of truth.
    /// Returns the new profile's name, or `None` when the original profile is
    /// not selected (edits then apply in place).
    ///
    /// Prefer `Project::fork_for_edit_with_event` so callers get a granular
    /// `ModelEvent`; this bare method exists for tests/internal use.
    pub fn fork_for_edit(&mut self) -> Option<String> {
        if self.selected_id() == self.original_id() {
            let name = self.next_available_name();
            let forked = self.add(name.clone());
            self.select(forked);
            return Some(name);
        }
        None
    }

    /// Rename a profile by id. Returns false if id not found.
    pub fn rename(&mut self, id: ProfileId, new_name: impl Into<String>) -> bool {
        if let Some(p) = self.profiles.iter_mut().find(|p| p.id == id) {
            p.name = new_name.into();
            true
        } else {
            false
        }
    }

    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }

    /// Monotonic next id.
    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    /// Reconstruct from raw parts (for persistence).
    pub fn from_raw(profiles: Vec<Profile>, selected: ProfileId, next_id: u64) -> Self {
        Self { profiles, selected, next_id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_is_resolved_through_selected_profile() {
        let mut profiles = Profiles::default();
        let jp = profiles.add("JP");
        let entry = EntryId(1);

        assert_eq!(profiles.selected().translation_of(entry), None);
        profiles.selected_mut().set_translation(entry, Some("こんにちは".into()));
        assert_eq!(profiles.selected().translation_of(entry), Some("こんにちは"));

        profiles.select(jp);
        assert_eq!(profiles.selected().name, "JP");
        assert_eq!(profiles.selected().translation_of(entry), None);
    }

    #[test]
    fn empty_delta_is_pruned() {
        let mut profiles = Profiles::default();
        let entry = EntryId(1);
        profiles.selected_mut().set_translation(entry, Some("hi".into()));
        profiles.selected_mut().set_translation(entry, None);
        assert!(profiles.selected().delta(entry).is_none());
    }

    #[test]
    fn cannot_remove_selected_or_last_profile() {
        let mut profiles = Profiles::default();
        let jp = profiles.add("JP");
        assert!(profiles.remove(jp));
        let default_id = profiles.selected_id();
        assert!(!profiles.remove(default_id));
        assert_eq!(profiles.len(), 1);
    }

    #[test]
    fn original_id_is_the_first_profile() {
        let mut profiles = Profiles::default();
        let original = profiles.original_id();
        assert_eq!(original, ProfileId(0));
        profiles.add("English");
        assert_eq!(profiles.original_id(), original);
    }

    #[test]
    fn next_available_name_counts_past_existing_profiles() {
        let mut profiles = Profiles::default();
        assert_eq!(profiles.next_available_name(), "Profile 1");
        profiles.add("Profile 1");
        assert_eq!(profiles.next_available_name(), "Profile 2");
        profiles.add("JP");
        profiles.add("Profile 2");
        profiles.add("Profile 5");
        assert_eq!(profiles.next_available_name(), "Profile 6");
        assert!(profiles.find_by_name("Profile 6").is_none());
    }

    #[test]
    fn next_available_name_ignores_foreign_numbers() {
        let mut profiles = Profiles::default();
        profiles.add("Profile alpha");
        profiles.add("Profile 12abc");
        assert_eq!(profiles.next_available_name(), "Profile 1");
    }

    #[test]
    fn fork_for_edit_forks_and_selects_when_original_is_selected() {
        let mut profiles = Profiles::default();
        let name = profiles.fork_for_edit().expect("fork must happen");
        assert_eq!(name, "Profile 1");
        assert_eq!(profiles.len(), 2);
        let forked = profiles.find_by_name(&name).unwrap();
        assert_ne!(forked, profiles.original_id());
        assert_eq!(profiles.selected_id(), forked);

        let entry = EntryId(1);
        profiles.selected_mut().set_translation(entry, Some("edited".into()));
        let original = profiles
            .iter()
            .find(|p| p.id == profiles.original_id())
            .expect("original profile always exists");
        assert_eq!(
            original.translation_of(entry),
            None,
            "the original profile must keep no delta"
        );
    }

    #[test]
    fn fork_for_edit_is_a_noop_on_non_original_profiles() {
        let mut profiles = Profiles::default();
        let jp = profiles.add("JP");
        profiles.select(jp);

        assert_eq!(profiles.fork_for_edit(), None);
        assert_eq!(profiles.len(), 2, "no new profile");
        assert_eq!(profiles.selected_id(), jp, "selection unchanged");
    }
}