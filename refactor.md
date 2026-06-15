# REFACTOR — De-bloat `src/app.rs` into a strict message channel

## Goal

`src/app.rs` is 2629 lines. ~850 lines of it are domain logic that belongs in the
crate that owns that domain (ocr, translation, styling, model, ui). After the
refactor, `app.rs` is only:

- the `Message` enum + `From<UiEvent>` (the message channel)
- the `App` struct (session fields + delegates to crate-owned session objects)
- `update()` as thin dispatch to crate APIs
- the `UiState` impl, `view()`, `subscription()`, `boot()`
- engine-readiness glue, status strings, surviving channel tests

Target: `app.rs` ~1200-1400 lines, every crate owns its own state and logic,
the tree compiles and all tests pass **after every session**.

## Hard invariants (do not break)

1. **Keep the tree green after every session**: `cargo check` and `cargo test --workspace` must pass. End every session with a commit.
2. **Zero behavior change.** No user-visible string changes, no event reordering, no logic rewrites. This is a *mechanical* move.
3. `ocr` crate stays **iced-free** (only model + rapidocr-core + image). No iced `Task` types in ocr/translation/styling/model.
4. Do NOT modify the vendored `iced/` directory.
5. Do NOT change `ui/src/event.rs` (the `UiEvent` channel shape stays).
6. Line numbers below are **as of the audit (app.rs @ 2629 lines)**; they shift as sessions delete code. Grep for function names, not line numbers.
7. If a session runs out of budget, finish it cleanly (tree green, tests green), commit, and write the remaining work into the **"Session handoff"** section of the NEXT session before stopping.

## Repo layout (current)

- `src/app.rs` (2629 lines) — iced app: `App`, `Message`, `update`, `view`, `subscription`, `boot`, unit tests.
- `src/settings.rs` (142 lines) — persisted `Settings` (connections, last_provider, auto_style_detect, ocr_workers, free_models_only). **Stays in the root crate.**
- `src/main.rs` (13 lines) — iced application wiring.
- `model/` — pure std-only data model: `Project`, `Profiles`, `OcrResult`, `EntryStyle`, `Quad`, `Extras`.
- `ocr/` — rapidocr wrapper: `Engine`, `ParallelEngine`, run planning (`plan_runs`), canvas stitching, merge/resolve/dedup/distribute. Already has every primitive; missing the assembly glue that app.rs re-wires.
- `translation/` — rig translation: catalog, `Connection`, `Provider`, fetch, `translate_all`, wire-format parse/align.
- `inpaint/` — LaMa engine, `run_blocking`.
- `styling/` — ONNX style classifier, `Engine`, `StylePrediction::to_entry_style`.
- `ui/` — iced widgets: `UiState` trait, `UiEvent`, `LoadedImage`, `PageDecode`/`Tier`, panel (incl. `PANEL_LIST_ID`, `panel_row_id`), toolbar, settings modal, connect modal.
- `iced/`, `rapidocr-core/`, `NeverLiieIcedWidgets/` — vendored / widgets. Never modify `iced/`.

## Crate dependency rules (what may reference what)

- `model`: nothing (std only).
- `ocr` → model. No iced, no tokio.
- `translation` → model (already). No iced, no tokio (it has rig/reqwest).
- `inpaint` → model. No iced.
- `styling` → model. No iced.
- `ui` → model, translation, iced (already), and will gain a `tokio` dep (session 5).
- root `scanlateit` → everything + iced + tokio + rfd + serde.

---

# Session 0 — Baseline

**Goal:** green tree + audit snapshot to measure against.

Status: **DONE (2026-08-18)** — commit `SESSION0 baseline`.

1. **Baseline results**
   - `cargo check` — PASS (only pre-existing warnings: dead code in ui overlay, unused `FetchModels` variant, unused `field` in app.rs).
   - `cargo test --workspace` — **root + 6 crates: all pass (180 tests).** The only failures are the vendored `NeverLiieIcedWidgets` doctests (13, pre-existing: `Element<'_, Message>` without a borrowed input, crate-name typo `neverlie_` vs `neverliie_`, E0283 in color_picker) plus its lib failing to build standalone (its iced dep lacks the `canvas` feature). **Per owner decision: leave `NeverLiieIcedWidgets` as-is; it is not part of the refactor.** Future sessions must verify the 6 crates + root only, and treat the neverlie doctest failures as noise. (Fixed en route: 3 dead-on-arrival tests — `applying_an_empty_preset_slot_is_a_noop` in app.rs, `svd2_reconstructs_the_matrix` + `transform_maps_box_corners_onto_the_skewed_quad` in ui — all failed since the commits that introduced them; production code untouched, test-only fixes.)
2. **Line counts (as of baseline):**
   - `src/app.rs` **2490** (audit said 2629 — drifted since; re-baselined here)
   - `src/main.rs` 11, `src/settings.rs` 130
   - `model/lib.rs` 23 (+ project 202, style 36, profile 214, entry 70)
   - `ocr/lib.rs` 1462, `translation/lib.rs` 967, `inpaint/lib.rs` 574, `styling/lib.rs` 426, `ui/lib.rs` 18
   - All line numbers in the session plans below were as-of-audit (2629); **grep for names, not lines.**
3. `git status` — tracked tree clean before the session; untracked: `iced/`, `rapidocr-core/`, `NeverLiieIcedWidgets/`, `models/*.onnx`, `settings.json`, `scanlateit.exe`, `refactor.md`, `CONTINUE.md`, `lineconfig.yaml`, `true(well, close enough)free-transform.md`. Committed: session-0 commit (2 test fixes + this file).
4. **Smoke test (manual, pending):** models present under `models/`. GUI smoke (OCR a few pages → translate → inpaint → scroll) is interactive and out of session scope; Session 7 does the full manual pass.

**Handoff note:** if any baseline test fails, STOP — the refactor starts from green. The green set = root + model/ocr/translation/inpaint/styling/ui (neverlie doctests excluded per decision).

---

# Session 1 — `model` crate

**Goal:** add 4 model APIs + tests, rewire the app handlers that touch them.
Deletes ~85 lines of logic and ~100 test lines from app.rs.

Status: **DONE (2026-08-18)** — commit `SESSION1 model crate APIs`.

1. **Model APIs added (all moved verbatim, zero behavior change):**
   - `model/src/profile.rs` — `Profiles::fork_for_edit` (the EditAction fork block).
   - `model/src/style.rs` — `INITIAL_PRESET_SLOTS` (8) + `StylePresets` (`default_presets` = the 5 seeded variants + 3 empty slots, `get`/`len`/`as_slice`/`is_empty`/`add`/`replace`/`remove`).
   - `model/src/entry.rs` — `Quad::intersects_rect` (the old `quad_intersects_rect` body, using `self.bounds()`).
   - `model/src/project.rs` — `Project::store_translation` (ensure-profile-by-name → select → set translation → id).
   - `model/src/lib.rs` — re-exports `StylePresets` + `INITIAL_PRESET_SLOTS`.
2. **Tests:** model crate now 32 tests (was 20): `seeded_presets_cover_the_expected_variants` + `default_style_round_trips_all_fields` moved from app.rs; new `add`/`replace`/`remove`/`get` rule tests; `fork_for_edit` (fork+select, original keeps no delta; no-op on non-original); `intersects_rect` (overlap/disjoint/edge/contained); `store_translation` (create+reuse+select, OCR untouched).
3. **app.rs rewired:** `style_presets` field → `presets: StylePresets`; `App::new` seeds via `StylePresets::default_presets()`; `UiState::style_presets()` → `self.presets.as_slice()`; `StylePresetApply/Add/Replace/Remove` → `presets.get/add/replace/remove`; `EditAction` fork block → `fork_for_edit()`; `InpaintSelection` → `quad.intersects_rect(rect)` (free fn deleted); `TranslateFinished` loop → `store_translation` (per-image, no `current_image` dedup — idempotent). Deleted `INITIAL_PRESET_SLOTS` const (imported from model in tests only) and the two moved tests. Channel-level preset/fork tests stay in app.rs (adapted to the new field).
4. **Line counts (now):** `src/app.rs` **2549** (was 2630 at the session-0 commit — the refactor.md "2490" baseline number was stale; net -81 this session), `model/style.rs` 212, `model/profile.rs` 295, `model/entry.rs` 120, `model/project.rs` 281.
5. **Verification:** `cargo check` clean (only pre-existing warnings: dead code in ui overlay, unused `FetchModels`, unused `field`); `cargo test -p scanlateit-model` 32 pass; `cargo test -p scanlateit` 23 pass; `cargo test --workspace` — root + 6 crates all pass (194 tests), only the pre-existing neverlie doctest noise (13).

**Handoff note:** nothing outstanding; session 2 (`translation` crate) can start from green.

---

### 1a. New APIs

**`model/src/profile.rs`** — add to `impl Profiles`:

```rust
/// Forks a fresh profile off the original ("Default") profile and selects it,
/// for inline edits that must not touch the OCR source of truth. Returns the
/// new profile's name, or `None` when the original profile is not selected
/// (edits then apply in place).
pub fn fork_for_edit(&mut self) -> Option<String>
```

Implementation is exactly the current block in app.rs (EditAction handler):
`if selected_id() == original_id() { let name = next_available_name(); let forked = add(name.clone()); select(forked); return Some(name); } None`.

**`model/src/style.rs`** — new type + const:

```rust
/// How many preset slots the app starts with: five built-in styles plus
/// three empty slots.
pub const INITIAL_PRESET_SLOTS: usize = 8;

/// The style-preset slot list shown in the styling panel, in memory only:
/// `None` = empty slot. "+" fills the first empty slot or appends; clicking
/// a filled swatch applies its style; right-click replaces or empties.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StylePresets(Vec<Option<EntryStyle>>);

impl StylePresets {
    pub fn default_presets() -> Self            // the 5 seeded variants + 3 empty (move App::new block verbatim)
    pub fn get(&self, index: usize) -> Option<EntryStyle>   // None for empty slot / out of range
    pub fn len(&self) -> usize
    pub fn as_slice(&self) -> &[Option<EntryStyle>]
    pub fn is_empty(&self) -> bool
    /// Fills the first empty slot, or appends when all are full.
    pub fn add(&mut self, style: EntryStyle)
    /// Overwrites slot `index` (no-op when out of range).
    pub fn replace(&mut self, index: usize, style: EntryStyle)
    /// Empties slot `index` (no-op when out of range).
    pub fn remove(&mut self, index: usize)
}
```

The seeded presets (current `App::new`, app.rs:312-328): white bg/black text, inverse, transparent bg/black text, transparent bg/white text, red bg/white text, then `resize(INITIAL_PRESET_SLOTS, None)`.

**`model/src/entry.rs`** — add to `impl Quad`:

```rust
/// Whether this quad's AABB overlaps `rect` (`[x, y, w, h]` in the same
/// coordinate space).
pub fn intersects_rect(&self, rect: [f32; 4]) -> bool
```

Body: the current `quad_intersects_rect` (app.rs:1292-1295), using `self.bounds()`.

**`model/src/project.rs`** — add to `impl Project`:

```rust
/// Ensures a profile named `profile_name` exists, selects it and sets the
/// entry's translated text in it. Returns the profile id.
pub fn store_translation(
    &mut self,
    profile_name: &str,
    entry_id: EntryId,
    text: Option<String>,
) -> ProfileId
```

Body: `find_by_name(...).unwrap_or_else(|| add(profile_name))`, `select(id)`, `selected_mut().set_translation(entry_id, text)`, return id.

### 1b. `model/src/lib.rs` exports

Add `StylePresets` to the re-exports.

### 1c. Tests to add in model

- `style.rs`: seeded presets exactly match the current app.rs test `seeded_presets_cover_the_expected_variants` (MOVE it verbatim, adapted to `StylePresets::default_presets()`); `add` fills first empty; `add` appends when full; `add` refills an emptied slot before appending; `replace` overwrites filled and empty slots, no-op out of range; `remove` empties a slot, no-op out of range; `get` returns None for empty/out-of-range. (These are the rules currently asserted by app.rs tests `add_preset_*`, `replace_preset_*`, `remove_preset_*` — the channel-level copies in app.rs may STAY, they test `update()` dispatch.)
- `profile.rs`: `fork_for_edit` returns Some + forks+selects when original selected (assert name = "Profile 1", len +1, selected changed, original still has no delta); returns None and doesn't touch profiles when a non-original profile is selected.
- `entry.rs`: `intersects_rect` true/false cases (overlap, disjoint, edge-touching, fully-contained).
- `project.rs`: `store_translation` creates a profile by name when missing and returns its id; reuses an existing one; sets the translation; does not create when the profile exists.

### 1d. Rewire `src/app.rs`

- `App::new`: replace the preset seeding block (app.rs:312-328) with `presets: StylePresets::default_presets()`. Delete `INITIAL_PRESET_SLOTS` const from app.rs (moved to model).
- Field `style_presets: Vec<Option<EntryStyle>>` → `presets: StylePresets`. All users:
  - `UiState::style_presets()` (app.rs:562) → `&self.presets` must return `&[Option<EntryStyle>]`; `StylePresets` needs a `pub fn as_slice(&self) -> &[Option<EntryStyle>]` (see 1a) or implement `Deref<Target = [Option<EntryStyle>]>` — pick `as_slice` + `len` to stay explicit.
  - `StylePresetApply` (app.rs:2465-2473): `app.presets.get(preset)` instead of `app.style_presets.get(preset).copied()` — mind the double-Option: `get` already returns `Option<EntryStyle>` (None for empty slot), so the handler's `Some(Some(...))` pattern simplifies to `Some(...)`.
  - `StylePresetAdd` (app.rs:2474-2481) → `app.presets.add(app.style_working); Task::none()`.
  - `StylePresetReplace` (app.rs:2482-2487) → `app.presets.replace(preset, app.style_working);`.
  - `StylePresetRemove` (app.rs:2488-2493) → `app.presets.remove(preset);`.
- `EditAction` handler (app.rs:2386-2403): replace the fork block with

```rust
if !app.editing_dirty {
    app.editing_dirty = true;
    let project = &mut app.images[index].project;
    if let Some(name) = project.profiles.fork_for_edit() {
        app.status = format!(
            "Edit forked into '{name}': the OCR text stays untouched."
        );
    }
}
```

  (Borrow check: `fork_for_edit` takes `&mut self` on profiles only — the `project` borrow is fine as today.)
- `InpaintSelection` handler (app.rs:2357): `quad_intersects_rect(quad, rect)` → `quad.intersects_rect(rect)`. Delete the free function (app.rs:1292-1295).
- `TranslateFinished` handler (app.rs:2557-2575): replace the ensure-profile/select/set block inside the loop with

```rust
let image = &mut app.images[*image_index];
image
    .project
    .store_translation(&profile_name, *entry_id, Some(translation.clone()));
```

  (The `current_image` dedup for `profile_name` lookup can be dropped: `store_translation` is idempotent per image. Keep `profile_name` computed once before the loop — it comes from `translation::profile_name(&app.translate_lang)`.)
- Delete moved test helpers/consts. The app.rs tests for presets/fork that run through `update()` stay (they now exercise the channel + model API together).

### 1e. Verification

```powershell
cargo check
cargo test -p scanlateit-model
cargo test -p scanlateit
```

Smoke: edit a line (fork still happens once), apply/add/replace/remove presets, inpaint a range, translate.

**Handoff note (if session runs out):** only `store_translation` rewiring may remain; the rest must be done and committed. Note exactly which handler in app.rs still uses the old inline code so the next session can finish it.

---

# Session 2 — `translation` crate

**Goal:** `translation::Session` session object + 2 free functions + tests; rewire all translate/connect state handling.
Deletes ~90 lines + ~40 test lines from app.rs.

Status: **DONE (2026-08-18)** — commit `SESSION2 translation session`.

1. **New `translation/src/session.rs` (402 lines):** `Session` (connections, selected_id, connected_ids, fetched, models, selected_model, free_only) with `new`/`sync`/`sync_models`/`connect`/`disconnect`/`select`/`set_free_only`/`on_fetched`/`fetch_ids`/`selected_provider`/`selected_api_key`/`is_connected`. All bodies ported verbatim from the deleted app.rs functions. One deliberate note: `sync` calls `sync_models` unconditionally (the old `sync_translate_providers` only did so when the selection changed) — required so the plan's `boot` wiring (`free_only` set, then `sync()`) applies the free-only filter; recomputation is deterministic so every existing call path observes identical results.
2. **Free functions in `translation/src/lib.rs`:** `file_tag` (moved `file_name` verbatim), `validate_connection` (the three messages ported verbatim: `"Enter an API key."`, `"Enter a base URL."`, `"Enter a model id."`; first error wins). Added `pub mod session; pub use session::Session;` (Cargo.toml unchanged).
3. **Tests:** translation crate now **34** (was 19): 13 `session` tests (catalog-then-custom ordering, fallback-to-first, last_provider pick/ignore, empty fallback, disconnect reselect + fetched eviction, connect select, select no-op rules, models rebuild on fetch/connect/select/free-only, fetch_ids excludes customs + empty, selected_provider custom/builtin/None, is_connected) + 2 lib tests (`file_tag` both separator styles; `validate_connection` first-error order incl. blank-key-over-custom-fields).
4. **app.rs rewired:** fields `connections`/`translate_provider`/`translate_providers`/`translate_providers_map`/`translate_model`/`translate_models`/`free_models_only` → one field `tx: translation::Session` (`translate_lang` stays). Deleted `file_name`, `sync_translate_providers`, `sync_translate_models`. `boot()` → `Session::new(settings.connections, settings.last_provider)`, `free_only`/`sync()` from settings, fetch via `tx.fetch_ids()`. Handlers: `FetchModels`/`ModelsFetched` → `fetch_ids()`/`on_fetched()`; `Translate` → `is_connected()` guard + `selected_provider()`/`selected_api_key()` + `file_tag()`; `TranslateProvider` → `tx.select(id)`; `TranslateModel` → `tx.selected_model = model`; `TranslateDisconnect` → `tx.disconnect(&id)` (kept the fetched-entry eviction inside `disconnect`); `ConnectModalSubmit` → `validate_connection` then `tx.connect(id, connection)`; `FreeModelsOnlyToggle` → `tx.set_free_only(enabled)`; `SettingsClose` → Settings from `tx.connections`/`tx.selected_id`/`tx.free_only`. `TranslateConnect` reads `app.tx.connections` for the prefill. UiState impl reads `tx.selected_id`/`connected_ids`/`models`/`selected_model`/`connections`/`free_only`.
5. **Line counts (now):** `src/app.rs` **2423** (was 2549; net -126), `translation/lib.rs` 1095 (+2 fns, tests, module decl), `translation/session.rs` 402.
6. **Verification:** `cargo check` clean (only pre-existing warnings: ui overlay dead code, unused `FetchModels`, unused `field`); `cargo test -p scanlateit-translation` 34 pass; `cargo test -p scanlateit` 23 pass; `cargo test --workspace` — root + 6 crates all pass (211 tests: 37 neverlie lib + 23 root + 18 inpaint + 32 model + 44 ocr + 10 styling + 34 translation + 13 ui), only the pre-existing neverlie doctest noise (13).

**Handoff note:** nothing outstanding; session 3 (`ocr` crate — the big one) can start from green.

### 2a. New file `translation/src/session.rs`

```rust
//! The connected-provider session: which connections exist, which one is
//! selected, the model picker lists and the free-only filter. The app stores
//! one of these and forwards `UiEvent`s to it; the crate owns all rules
//! (catalog ordering, selection fallback, model list sync).

use std::collections::{BTreeMap, HashMap};
use super::{Connection, Provider, catalog_provider, custom_fallback_provider,
            is_custom, provider_for_connection, CUSTOM_OPENAI, CUSTOM_ANTHROPIC,
            SUPPORTED_PROVIDERS};

#[derive(Debug, Clone, Default)]
pub struct Session {
    /// Stored connections, keyed by provider id; connected == has entry.
    pub connections: BTreeMap<String, Connection>,
    /// The selected provider id; always one of `connected_ids` when non-empty.
    pub selected_id: String,
    /// Connected ids in catalog order, then the custom slots.
    pub connected_ids: Vec<String>,
    /// Fetched gateway configs from the models mirror, keyed by id.
    pub fetched: HashMap<String, Provider>,
    /// The model picker entries of the selected provider.
    pub models: Vec<String>,
    /// The selected model id; always one of `models` when non-empty.
    pub selected_model: String,
    /// Free-only filter for the model picker.
    pub free_only: bool,
}

impl Session {
    /// Restores the stored connections, then picks `last_provider` when it is
    /// still connected (or falls back to the first connected provider).
    pub fn new(connections: BTreeMap<String, Connection>, last_provider: Option<String>) -> Self
    /// Rebuilds `connected_ids` (catalog order + custom slots) and fixes
    /// `selected_id` when it dropped out (falls back to the first connected
    /// provider, or empty). Calls `sync_models`.
    pub fn sync(&mut self)
    /// Rebuilds `models`/`selected_model` for the current provider.
    pub fn sync_models(&mut self)
    /// Stores a connection and selects it; `sync`s.
    pub fn connect(&mut self, id: String, connection: Connection)
    /// Removes a connection; `sync`s.
    pub fn disconnect(&mut self, id: &str)
    /// Selects `id` (only when connected); `sync_models`s.
    pub fn select(&mut self, id: String)
    /// Sets the free-only filter; `sync_models`s.
    pub fn set_free_only(&mut self, free_only: bool)
    /// Merges fetched listings; `sync_models`s.
    pub fn on_fetched(&mut self, providers: HashMap<String, Provider>)
    /// The ids that need a models fetch (connected, non-custom).
    pub fn fetch_ids(&self) -> Vec<String>
    /// The requestable [`Provider`] for the selected connection (catalog or
    /// custom, with the connection's api/kind/model baked in).
    pub fn selected_provider(&self) -> Option<Provider>
    /// The stored API key of the selected connection, if any.
    pub fn selected_api_key(&self) -> Option<String>
    pub fn is_connected(&self) -> bool
}
```

Semantics to port **verbatim** (do not improve):
- `sync` = current `sync_translate_providers` (app.rs:419-436): iterate `SUPPORTED_PROVIDERS` for connected, then `[CUSTOM_OPENAI, CUSTOM_ANTHROPIC]`; rebuild `connected_ids`; if `selected_id` no longer in list → first id or empty → `sync_models`.
- `sync_models` = current `sync_translate_models` (app.rs:443-471): empty selected → clear; provider = `fetched.get(selected_id)` else `provider_for_connection(selected_id, connection)`; `selectable_models(free_only)`; empty → keep; reset `selected_model` when not in list.
- `selected_provider` = the `(provider, api_key)` extraction in the `Translate` handler (app.rs:2106-2116): `connections.get(&selected_id).map(|c| (provider_for_connection(...), c.api_key.clone()))`.

**New free functions in `translation/src/lib.rs`:**

```rust
/// The last path component of an image path; the file tag of the wire format.
pub fn file_tag(path: &str) -> String        // move file_name (app.rs:343-345)

/// First validation error of the connect modal form, if any:
/// - api_key must be non-blank;
/// - custom connections also need a base URL and a model id.
pub fn validate_connection(
    is_custom: bool, api_key: &str, base_url: &str, model: &str,
) -> Option<String>
```

Port the messages verbatim: `"Enter an API key."`, `"Enter a base URL."`, `"Enter a model id."`.

### 2b. `translation/src/lib.rs` exports + tests

- `pub mod session; pub use session::Session;` (add to lib.rs; Cargo.toml unchanged).
- Tests:
  - `session.rs`: sync orders catalog-then-custom; selection falls back to first connected; falls back to empty; models rebuild on connect/select/free-only toggle; `on_fetched` merges and triggers sync; `fetch_ids` excludes customs; `selected_provider` resolves custom api/model and builtin catalog; `is_connected`.
  - `lib.rs`: `file_tag` strips `C:\a\b\c.png` and `/a/b/d.png` → basename; `validate_connection` returns None for a valid builtin (key only), Some for blank key, Some for custom missing URL, Some for custom missing model, in that order (first error wins), None when custom is complete.

### 2c. Rewire `src/app.rs`

- Replace the fields `connections`, `translate_provider`, `translate_providers`, `translate_providers_map`, `translate_model`, `translate_models`, `free_models_only` (app.rs:189-208) with one field:

```rust
pub(crate) tx: translation::Session,
```

  Delete `sync_translate_providers` (419-436) and `sync_translate_models` (443-471), and `file_name` (343-345).
- `App::new`: `tx: translation::Session::default()`.
- `boot()` (1014-1046): `app.tx = translation::Session::new(settings.connections, settings.last_provider); app.tx.free_only = settings.free_models_only; app.tx.sync();` then the fetch task uses `app.tx.fetch_ids()`.
- `UiState` impl (495-528, 549...): `translate_provider()` → `translation::provider_name(&self.tx.selected_id)`; `translate_providers()` → `self.tx.connected_ids.iter().map(provider_name)`; `translate_model()` → `&self.tx.selected_model`; `translate_models()` → `&self.tx.models`; `connections()` → `&self.tx.connections`; `free_models_only()` → `self.tx.free_only`.
- Handlers:
  - `FetchModels` (1691-1703): `let ids = app.tx.fetch_ids();` then the same perform.
  - `ModelsFetched` (1704-1708): `app.tx.on_fetched(providers);`.
  - `Translate` (2064-2133): `app.tx.is_connected()` guard; jobs unchanged; `let (provider, api_key) = match app.tx.selected_provider() { Some(p) => (p, app.tx.selected_api_key()), None => { app.translating = false; status...; return Task::none(); } };` model = `app.tx.selected_model.clone()`; status string uses `provider.name` as today.
  - `TranslateProvider(name)` (2134-2147): map display name → id as today, then `app.tx.select(id)`.
  - `TranslateModel(model)` (2148-2151): `app.tx.selected_model = model;`.
  - `TranslateConnect` (2156-2170): unchanged (reads `app.tx.connections` for existing values).
  - `TranslateDisconnect` (2171-2180): `app.tx.disconnect(&provider_id);` keep status string.
  - `ConnectModalSubmit` (2202-2250): validation → `translation::validate_connection(modal.is_custom, &modal.api_key, &modal.base_url, &modal.model)` and on Some, re-open the modal with `error: Some(e)`; on success `app.tx.connect(id, connection);` status string as today; custom → `Task::none()` else the fetch-perform with `vec![id]`.
  - `FreeModelsOnlyToggle` (2255-2261): `app.tx.set_free_only(enabled);`.
- `SettingsClose` (2528-2543): build `Settings` from `app.tx.connections`, `last_provider: (!app.tx.selected_id.is_empty()).then(|| app.tx.selected_id.clone())`, `free_models_only: app.tx.free_only` (unchanged semantics).

### 2d. Verification

```powershell
cargo check
cargo test -p scanlateit-translation
cargo test -p scanlateit
```

Smoke: boot with a stored `settings.json` (connected provider restored, model list fetched), switch provider, connect/disconnect, free-only toggle, translate.

**Handoff note:** if time runs out, `Session` + tests must be done (they are self-contained); the app rewiring can be the next session's first step — list which handlers still need re-wiring.

---

# Session 3 — `ocr` crate

**Goal:** `RunEvent`, `assemble`, `RunSession`, commit helpers + tests; app keeps only the ~20-line channel pump.
Deletes ~350 lines from app.rs. This is the largest session; if it overruns, leave `RunSession` for the next session and commit the rest.

Status: **DONE (2026-08-18)** — commit `SESSION3 ocr session`.

1. **New `ocr/src/session.rs` (225 lines):** `RunEvent` with `index` stamped into both variants (`Canvas { index, width, margin_top, lines }`, `Fallback { index, result }`); `BuiltCanvas`/`build_canvas` moved verbatim from app.rs (pub(crate)); `RunSession` (`new` + `step`) — the `start_ocr_stream` bookkeeping ported exactly: fill the `workers + 1` window, fallback results returned immediately without entering the pipeline, ordered `recv` pops `canvas_meta` in submission order, `Ok(None)` when done, errors/`"cancelled"` propagate.
2. **`ocr/src/lib.rs` additions:** `assemble` (ported `assemble_run` verbatim, incl. the `eprintln!` debug line; signature `(index, width, margin_top, lines, plans, dims, held, prev)`), `RunResult::commit_entries(&self, &mut [Project])`, `BoundaryState::commit(&self, &mut [Project])`, `pub mod session` + `pub use session::{RunEvent, RunSession}`.
3. **Tests:** ocr crate now **51** (was 44): `assemble` whole-page/chunked mapping, held-candidate resolution across runs, dedup against prev-page quads; `commit_entries` counts + out-of-range skip; `BoundaryState::commit` same; `RunSession::new` bookkeeping (`total`/`window = workers + 1`) with a `// NOTE` that the pump path is covered by the app-level OCR smoke + rapidocr-core e2e (no engine fakes, per plan). One test assertion fixed en route: `assemble` merges lines first, so a band-run's entries arrive top-to-bottom (`edge` before `chunk`).
4. **app.rs rewired:** `Message::OcrStreamRun(Result<ocr::RunEvent, String>)` (index now inside the event); deleted `OcrRunOutcome`, `CanvasBuild`, `build_canvas`, `dispatch_run`, `assemble_run`, `commit_run_result`, and the `VecDeque` import; `start_ocr_stream` is now the ~20-line pump (collect paths → `RunSession::new` → forward events). `OcrStreamRun` handler: Canvas arm builds `prev` from `run.dedup` then calls `ocr::assemble(...)` and commits via a new tiny `commit_per_page` helper; Fallback arm flushes held boundary (`flush_held_boundary` kept) then commits. `maybe_start_ocr`/`finalize_run`/`OcrTick` subscription unchanged.
5. **Two deliberate deviations from the plan's sketch:** (a) `commit_entries`/`BoundaryState::commit` take `&mut [Project]`, which app.rs cannot produce from `Vec<LoadedImage>` (`LoadedImage` is not a `Project` wrapper); the plan's own fallback — "a tiny local helper or the loop inline over `app.images.iter_mut()`" — is used instead (`commit_per_page`, existing `flush_held_boundary`); the ocr APIs exist and are tested for future use. (b) `iced::stream::try_channel`'s closure param needs an explicit `Sender<Message>` annotation (the smaller pump body no longer pins the sender type). Also dropped the old `eprintln!("[ocr-stream] sent run...")` send-timing debug line (not user-visible; the plan's pump omits it).
6. **Line counts (now):** `src/app.rs` **2165** (was 2423; net -258 this session), `ocr/lib.rs` 1858 (+281), `ocr/session.rs` 225 (new).
7. **Verification:** `cargo check` clean (only pre-existing warnings: ui overlay dead code, unused `FetchModels` variant, unused `field`); `cargo test -p scanlateit-ocr` 51 pass; `cargo test -p scanlateit` 23 pass; `cargo test --workspace` — root + 7 crates all pass (218 tests: 37 neverlie lib + 23 root + 18 inpaint + 32 model + 51 ocr + 10 styling + 34 translation + 13 ui), only the pre-existing neverlie doctest noise (13) + its standalone lib build failure.

**Handoff note:** nothing outstanding; the manual OCR smoke (multi-page set + very tall page + corrupt file + cancel mid-run) is interactive and stays for Session 7's full pass. Session 4 (`styling` crate — small) can start from green.

### 3a. New file `ocr/src/session.rs`

```rust
//! The whole planned run set driven to completion: windowed canvas
//! submission, ordered result delivery, undecodable-page fallback. Iced-free:
//! the app pumps `step()` and forwards the events to its message channel.

use std::collections::VecDeque;
use scanlateit_model::{NewEntry, Project, Quad};
use crate::{BoundaryState, Engine, OcrCancellationToken, OcrLine, ParallelEngine,
            RunPlan, RunResult, build_canvas, STITCH_MARGIN_RATIO};

/// One run's outcome as the UI must see it.
#[derive(Debug, Clone)]
pub enum RunEvent {
    /// Raw lines plus the canvas metrics the app needs for assembly.
    Canvas { width: u32, margin_top: u32, lines: Vec<OcrLine> },
    /// Fallback of the undecodable-page path: ready-to-commit per-page result.
    Fallback(RunResult),
}

/// Drives the planned run set with a bounded in-flight window (`workers + 1`).
pub struct RunSession {
    plans: Vec<RunPlan>,
    dims: Vec<(u32, u32)>,
    paths: Vec<Vec<String>>,
    above_paths: Vec<Option<String>>,
    below_paths: Vec<Option<String>>,
    window: usize,
    dispatched: usize,
    in_flight: usize,
    canvas_meta: VecDeque<(u32, u32)>,
    total: usize,
}

impl RunSession {
    pub fn new(
        plans: Vec<RunPlan>, dims: Vec<(u32, u32)>,
        paths: Vec<Vec<String>>, above_paths: Vec<Option<String>>,
        below_paths: Vec<Option<String>>, workers: usize,
    ) -> Self
    /// Advances the run set by one step: fills the submission window, then
    /// blocks on the pipeline's ordered `recv` when the window is full.
    /// Returns `None` when every run is done. May block (image loads +
    /// inference recv) — call from a background task/stream, never the UI.
    pub fn step(
        &mut self,
        pipeline: &ParallelEngine,
        fallback: &Engine,
        token: &OcrCancellationToken,
    ) -> Result<Option<RunEvent>, String>
}
```

`step` internal loop (port `start_ocr_stream`'s bookkeeping exactly):
1. If `dispatched < total && in_flight < window`: `build_canvas(...)` for run `dispatched`.
   - Ready canvas → `submit(index, canvas)`, `in_flight += 1`, push `(width, margin_top)` onto `canvas_meta`, `dispatched += 1`; loop.
   - Fallback result → `dispatched += 1`; return `Some(RunEvent::Fallback(result))` immediately (same as today: fallback results are sent without entering the pipeline).
   - Err → propagate.
2. If `dispatched == total && in_flight == 0`: return `Ok(None)`.
3. Else: `pipeline.recv()` → `(idx, lines)`, `in_flight -= 1`, pop `canvas_meta` front (same order as today: metadata pushed in submission order == recv order); return `Some(RunEvent::Canvas { width, margin_top, lines })`.
4. `pipeline.recv()` / submit errors → `Err(String)`; cancellation surfaces as `Err("cancelled")` from recv (unchanged).

**Move `build_canvas` + `CanvasBuild` into `ocr/src/session.rs`** (or lib.rs — pick session.rs since only RunSession uses it): port app.rs:1178-1238 verbatim, rename `CanvasBuild` → `BuiltCanvas` (pub within crate; both variants pub for the module), `pub fn build_canvas(fallback, token, index, run, paths, above_path, below_path, dims) -> Result<BuiltCanvas, String>`. It needs `load_rgb`, `body_height`, `top_margin_strip`, `bottom_margin_strip`, `stack_run`, `STITCH_MARGIN_RATIO`, `to_entries` from lib.rs — all already `pub`.

### 3b. `ocr/src/lib.rs` additions

```rust
/// Assembles one run's raw lines into a commit-ready [`RunResult`], strictly
/// in run order on the UI thread: merges nearby boxes, resolves the previous
/// run's held boundary candidates against this run's re-detections in its top
/// margin, dedups against the committed quads of the page above, distributes
/// the survivors to their pages and holds this run's own boundary candidates
/// for the next run.
///
/// `prev` is the dedup target: the committed quads of the page above, its
/// width and the offset of this run's canvas top edge in that page's pixel
/// space (`(quads, prev_width, prev_offset)`).
pub fn assemble(
    index: usize,
    width: u32,
    margin_top: u32,
    lines: Vec<OcrLine>,
    plans: &[RunPlan],
    dims: &[(u32, u32)],
    held: Option<BoundaryState>,
    prev: Option<(Vec<Quad>, u32, u32)>,
) -> RunResult
```

Port `assemble_run` (app.rs:1400-1467) verbatim: `run = plans[index]`; `run_dims` from `dims`; `prev_held = held`; `prev_data` from `prev` (map into `(quads, prev_width, offset)`); merge → resolve/transform → dedup → distribute → fold resolved appends into per_page → sort → held wrap. Keep the `eprintln!` debug line.

```rust
impl RunResult {
    /// Appends this run's per-page entries to the projects (indexed by page)
    /// and returns the appended count.
    pub fn commit_entries(&self, projects: &mut [Project]) -> usize
}
impl BoundaryState {
    /// Appends every held candidate to its page's project and returns the
    /// appended count. Used when a run fails, is cancelled or never starts:
    /// the captured bubbles must not be lost.
    pub fn commit(&self, projects: &mut [Project]) -> usize
}
```

(`Project::append_ocr` is the model API; both are one-line loops.)

### 3c. Tests (ocr crate)

- `assemble`: build synthetic `RunPlan`s + dims + lines and assert: single-run whole-page assembly produces per-page entries; boundary-held candidates of run 0 get resolved against run 1's re-detections (reuse the resolve_boundary fixture style already in lib.rs tests); dedup against `prev` quads drops the top-margin duplicates; a chunked (band) plan maps entries into the page. The existing lib.rs `distribute`/`resolve_boundary` tests give you the exact shapes.
- `RunSession`: windowing math without engines is hard (needs real `ParallelEngine`). Test what's testable: `new` bookkeeping (total = plans.len(), window = workers+1); document in a `// NOTE` comment that the pump path is covered by the app-level OCR integration smoke + the existing rapidocr-core e2e tests. **Do not** invent engine fakes.
- `commit_entries`/`BoundaryState::commit`: with a `Project::new()` per page, assert counts and appended entries.

### 3d. Rewire `src/app.rs`

- Delete `OcrRunOutcome` (111-128), `start_ocr_stream` (1057-1175), `CanvasBuild` (1178-1183), `build_canvas` (1190-1238), `dispatch_run` (1244-1288), `assemble_run` (1400-1467), `commit_run_result` (1471-1479), `flush_held_boundary` (1484-1492). Remove now-unused imports (`VecDeque`, `ocr::load_rgb`-related, `STITCH_MARGIN_RATIO` etc.).
- `Message::OcrStreamRun` payload: `Result<ocr::RunEvent, String>`.
- `start_ocr_stream` becomes:

```rust
/// Spawns the parallel OCR stream: the [`ocr::RunSession`] does the
/// windowing/ordering on the pipeline, this task only forwards its events
/// into the iced channel.
fn start_ocr_stream(app: &mut App) -> Task<Message> {
    let pipeline = app.pipeline.clone().expect("pipeline must be built");
    let fallback = app.engine.clone().expect("engine must be built");
    let token = app.cancel.clone().expect("cancellation token set");
    let runs = app.ocr_plans.clone();
    let dims = app.ocr_dims.clone();
    let paths: Vec<Vec<String>> = runs.iter()
        .map(|run| (run.page_start..=run.page_end).map(|i| app.images[i].path.clone()).collect())
        .collect();
    let above_paths: Vec<Option<String>> = runs.iter()
        .map(|run| run.above.map(|(page, _)| app.images[page].path.clone())).collect();
    let below_paths: Vec<Option<String>> = runs.iter()
        .map(|run| run.below.map(|(page, _)| app.images[page].path.clone())).collect();
    let workers = app.ocr_workers.parse::<usize>().unwrap_or(2).max(1);
    let mut session = ocr::RunSession::new(runs, dims, paths, above_paths, below_paths, workers);
    Task::stream(iced::stream::try_channel(1, move |mut sender| async move {
        loop {
            let Some(event) = session.step(&pipeline, &fallback, &token) else { break };
            let sent = sender
                .send(Message::OcrStreamRun(session_last_index(&event), Ok(event)))
                .await;
            if sent.is_err() { return Ok(()); }
        }
        Ok(())
    }).map(|item| match item {
        Ok(message) => message,
        Err(e) => Message::OcrStreamFailed(e),
    }))
}
```

  Note: the index for `OcrStreamRun` — both `RunEvent` variants carry/need an index; the current code sends `(idx, outcome)` pairs. If you make `RunEvent::Canvas`/`Fallback` carry the run index (add a field), the pump is simpler: `Message::OcrStreamRun(Ok(event))` with `event.index()`. **Decision: add `index: usize` to both variants** (`Canvas { index, width, margin_top, lines }`, `Fallback { index, result }`) — cleaner than the closure above; adjust `step` to stamp them.
- `OcrStreamRun` handler (1934-1980):
  - `Canvas` arm → `let run_result = ocr::assemble(index, width, margin_top, lines, &app.ocr_plans, &app.ocr_dims, app.held_boundary.take(), prev)` where `prev` is built from `run.dedup` (as `assemble_run` did: quads of the dedup page + `app.images[page].width as u32` + offset). Then `app.held_boundary = run_result.held; app.ocr_total += run_result.commit_entries(project_slice(app));` — write a tiny local helper or do the `(usize, Vec<NewEntry>)` loop inline over `app.images.iter_mut()`.
  - `Fallback` arm → `flush_held_boundary` becomes: `if let Some(state) = app.held_boundary.take() { app.ocr_total += state.commit(&mut projects) }` (use `BoundaryState::commit`).
  - `Err` arm unchanged.
- Keep `maybe_start_ocr` (1497-1515) and `finalize_run` (1517-1537) as-is (engine-readiness glue + status — the session's job).
- Keep the `OcrTick` subscription (2593-2599) unchanged.

### 3e. Verification

```powershell
cargo check
cargo test -p scanlateit-ocr
cargo test -p scanlateit
```

Smoke (critical — this session changes the OCR path): OCR a **multi-page set with a very tall page** (forces chunked runs + boundary candidates), verify: no duplicate bubbles across page seams, bubbles cut by the run boundary not lost, cancel works mid-run, undecodable page (drop a corrupt file into the folder) falls back without losing prior results.

**Handoff note:** if `RunSession::step` is unfinished, commit `RunEvent`/`assemble`/`commit_entries` rewiring first (the app can keep `start_ocr_stream` temporarily using `build_canvas` if you also port it — otherwise do NOT leave app.rs half-wired; revert the OCR part of app.rs to head if the session cannot finish it). The correct recovery: finish the session before committing app.rs changes for this crate.

---

# Session 4 — `styling` crate

**Goal:** `classify_entry` + `JobTracker` + tests; app keeps only the job enumeration + spawn.
Deletes ~65 lines from app.rs. Small session — a good catch-up slot.

Status: **DONE (2026-08-18)** — commit `SESSION4 styling job tracker`.

1. **`styling/src/lib.rs`:** added `Engine::classify_entry` (predict + map in one call, using `EntryStyle::default()` as the base — the app's old inline composition). Added `pub mod tracker; pub use tracker::JobTracker;`.
2. **New `styling/src/tracker.rs` (151 lines):** `JobTracker` with `new`/`engine`/`is_building`/`mark_building`/`set_engine`/`fail_build`/`is_done`/`mark_done`/`reopen`/`done_count`. **One deliberate deviation from the plan's sketch:** `JobTracker` is generic over the engine type (`JobTracker<E = Engine>`), because the plan's tests need an `Engine` value for `set_engine` and no engine fake is possible (Session 3's "no engine fakes" rule; `ort::Session` is not constructible without a model file and `zeroed()` panics on `Arc`-bearing types). The app's `JobTracker` default works exactly as sketched; tests use `JobTracker::<()>`. `Default` is hand-written (derived would impose `E: Default`).
3. **Tests:** styling crate now **15** (was 10): 5 new tracker tests (engine None until set; `set_engine` true only after `mark_building` + flag cleared either way; `fail_build` clears; done/reopen semantics + `done_count` + idempotent mark; pairs are distinct per index/id). Per plan, `classify_entry` itself is model-dependent → smoke-tested manually (a `// NOTE` documents this; the mapping `to_entry_style` is already unit-tested).
4. **app.rs rewired:** fields `styling_engine`/`styling_pending`/`styled: HashSet` → one field `styling: JobTracker`; `App::new` seeds `JobTracker::new()`; `classify_entries` → `app.styling.engine()` (clone) or `mark_building()` + build task; `start_style_jobs` filter → `!app.styling.is_done(index, entry.id)` (captures `&app.styling` — the `move` closure must not move `app`, same shape as before), up-front marking → `mark_done`, spawn-blocking body → `engine.classify_entry(&path, &quad)`; `StylingEngineReady` Ok → `if app.styling.set_engine(engine.clone()) { start_style_jobs(...) }` (returns the pending-build flag so queued jobs start exactly as the old `styling_pending` check), Err → `fail_build()`; `StyleDetected` → `mark_done` on success; `StyleAutoDetect` → `reopen` then `classify_entries`. `HashSet` no longer referenced in app.rs; the `StylingEngine` alias stays (message variant + `build()` call).
5. **Line counts (now):** `src/app.rs` **2155** (was 2165; net -10 — the plan's ~65-line estimate didn't account for the tracker field's doc comments and the import), `styling/lib.rs` 472 (+11), `styling/tracker.rs` 151 (new).
6. **Verification:** `cargo check` clean (only pre-existing warnings: ui overlay dead code, unused `FetchModels` variant, unused `field` in app.rs); `cargo test -p scanlateit-styling` 15 pass; `cargo test -p scanlateit` 23 pass; `cargo test --workspace` — root + 7 crates all pass (223 tests: 37 neverlie lib + 23 root + 18 inpaint + 32 model + 51 ocr + 15 styling + 34 translation + 13 ui), only the pre-existing neverlie doctest noise (13) + its standalone lib build failure.

**Handoff note:** nothing outstanding; session 5 (`ui` crate — `decode::Scheduler`, `panel::scroll_to_row`, `color::rgba_to_color`, tokio dep in ui only) can start from green. Smoke (auto-detect on after OCR, manual StyleAutoDetect re-run, model-file disconnect → status error) stays for Session 7's full manual pass.

### 4a. `styling/src/lib.rs` additions

```rust
impl Engine {
    /// Decodes `path`, classifies the `quad` crop and maps the prediction
    /// onto an [`EntryStyle`] in one call (the app's auto-detect job).
    pub fn classify_entry(&self, path: &str, quad: &Quad) -> Result<EntryStyle, String> {
        self.predict_entry(path, quad)
            .map(|pred| pred.to_entry_style(EntryStyle::default()))
    }
}
```

New file `styling/src/tracker.rs`:

```rust
//! Bookkeeping for auto style-detection jobs: the shared engine (built
//! lazily), the in-flight build flag, and the set of entries already
//! classified so an auto-run never classifies the same entry twice.

use std::collections::HashSet;
use scanlateit_model::EntryId;
use crate::Engine;

pub struct JobTracker {
    engine: Option<Engine>,
    building: bool,
    done: HashSet<(usize, EntryId)>,
}

impl JobTracker {
    pub fn new() -> Self
    /// A clone of the engine when it finished loading.
    pub fn engine(&self) -> Option<Engine>
    /// True when an engine build task is in flight.
    pub fn is_building(&self) -> bool
    /// Records that a build task was started.
    pub fn mark_building(&mut self)
    /// Stores the loaded engine. Returns true when a build was pending (the
    /// app starts the queued jobs).
    pub fn set_engine(&mut self, engine: Engine) -> bool
    /// Clears the building flag after a failed build.
    pub fn fail_build(&mut self)
    pub fn is_done(&self, index: usize, id: EntryId) -> bool
    pub fn mark_done(&mut self, index: usize, id: EntryId)
    /// Re-opens `(index, id)` so a manual StyleAutoDetect can rerun it.
    pub fn reopen(&mut self, index: usize, id: EntryId)
    /// The number of classified entries (for tests).
    pub fn done_count(&self) -> usize
}
```

### 4b. Tests

- `lib.rs`: `classify_entry` — no model file at test time, so test the *mapping* indirectly is impossible without the ONNX model; instead add a `#[cfg(test)]` unit for the wrapper only if cheap. Real coverage stays in `to_entry_style` (already tested). Note in the test module: model-dependent paths are smoke-tested manually.
- `tracker.rs`: engine None until set; `set_engine` returns true only when `mark_building` was called; `fail_build` clears; done/reopen semantics; `done_count`.

### 4c. Rewire `src/app.rs`

- Fields (161-170): replace `styling_engine: Option<StylingEngine>`, `styling_pending: bool`, `styled: HashSet<(usize, EntryId)>` with one field `styling: styling::JobTracker`.
- `classify_entries` (1326-1335):

```rust
fn classify_entries(app: &mut App) -> Task<Message> {
    match app.styling.engine() {
        Some(engine) => start_style_jobs(app, engine),
        None => {
            app.styling.mark_building();
            app.status = "Loading the styling model...".to_string();
            Task::perform(async move { StylingEngine::build() }, Message::StylingEngineReady)
        }
    }
}
```

- `start_style_jobs` (1340-1391): the filter becomes `filter(|entry| !app.styling.is_done(index, entry.id))`; the up-front marking loop → `app.styling.mark_done(*index, *id)`; the spawn-blocking body → `engine.classify_entry(&path, &quad)` (drop the manual `pred.to_entry_style(...)`); delete the `let styled = &app.styled;` comment block.
- `StylingEngineReady` (1865-1880): `if app.styling.set_engine(engine) { start_style_jobs(app, engine) } else { Task::none() }`; `Err` → `app.styling.fail_build(); status = e;`.
- `StyleDetected` (1881-1888): `app.styling.mark_done(index, id);`.
- `StyleAutoDetect` (2495-2501): `app.styling.reopen(index, id); classify_entries(app)`.
- Remove the `std::collections::HashSet` import if unused elsewhere.

### 4d. Verification

```powershell
cargo check
cargo test -p scanlateit-styling
cargo test -p scanlateit
```

Smoke: auto-detect on after OCR (every visible entry gets a style, no double scheduling), manual StyleAutoDetect re-runs the selected entry, disconnect of the model file → status error, no panic.

**Handoff note:** small session; if it overruns, only the app rewiring may remain — leave the `classify_entries`/`start_style_jobs` handlers listed as TODO in this section.

---

# Session 5 — `ui` crate

**Goal:** `decode::Scheduler`, `panel::scroll_to_row`, `color::rgba_to_color` + tests; app gains a `tokio` dep in ui only.
Deletes ~220 lines from app.rs.

Status: **DONE (2026-08-19)** — commit `SESSION5 ui scheduler + scroll + color`.

1. **`ui/Cargo.toml`:** added `tokio = { version = "1", features = ["rt", "time"] }` (direct dep for `spawn_blocking` + `time::sleep`; the root crate keeps its own tokio — still used by the inpaint/style spawn_blocking paths).
2. **`ui/src/main_area/decode.rs` (+251):** moved consts `DECODE_PRELOAD`, `SETTLE_DEBOUNCE`, `FULL_KEEP_MARGIN` (all `pub`) plus `Scheduler` (`new`/`full_window`/`schedule`/`accept_elapsed`/`settled`/`needs_settle`/`keep_full`/`settle`/`decode_thumbs`) ported verbatim from `full_window`/`decode_async`/`schedule_settle`/`settle_full` and the `ImagesPicked` thumb batch; private `decode_async` moved here too. **Two deliberate deviations from the plan's sketch:** (a) `schedule`'s future now returns the sequence number so `map: Fn(u64) -> T` fits `Task::perform` (plan's signature assumed a `()`-output future with an unused param); (b) `Scheduler`/`settle`/`decode_thumbs` gained `T: Send + 'static` bounds (iced `Task<T>` requires it — the E0277/E0310 fixes), and `needs_settle`'s `len` param is unused (`_len`) since the old `settled.contains` check never clamped (no image removal path exists).
3. **`ui/src/panel/results.rs` (+81):** `MeasurePanelRow` moved verbatim, genericized `impl<T: 'static> Operation<T>` (the compiler-demanded `'static` bound), and `pub fn scroll_to_row<T: Send + 'static>(index, id) -> iced::Task<T>` built via `operate`; reuses the module's `PANEL_LIST_ID`/`panel_row_id`.
4. **New `ui/src/color.rs` (29 lines):** `rgba_to_color` moved verbatim; re-exported via `pub mod color;` in lib.rs.
5. **Tests:** ui crate now **19** (was 13): `full_window` clamps at both ends; `needs_settle`; `keep_full` (window + preload held, far pages not, None settled → false); `accept_elapsed` (stale rejected, pending accepted); `settle` takes the pending range, records `settled()`, marks the window `Tier::Decoding`, keeps far pages `Absent` and no-ops on a second call (dummy `LoadedImage`s — the spawned tasks are never awaited, so no real decode happens); `rgba_to_color` channel + alpha mapping. Task-building glue per plan: not unit-tested.
6. **app.rs rewired:** fields `settle_seq`/`pending_settle`/`settled` → one field `scheduler: Scheduler`; deleted the three consts and `full_window`/`decode_async`/`schedule_settle`/`settle_full`/`MeasurePanelRow`/`panel_scroll_task`/local `rgba_to_color`. Handlers: `TilesVisible` → `scheduler.schedule(range, |seq| SettleElapsed(seq))`; `SettleElapsed` → `accept_elapsed` guard + `scheduler.settle(...)`; `TileScrollEnded` → `scheduler.settle(...)`; `FullDecoded` → `keep_full(len, index)`; `ImagesPicked` thumb batch → `scheduler.decode_thumbs(...)`; `select_entry` → `needs_settle` + `schedule`; `start_inline_edit`/`EntryClicked` → `scroll_to_row::<Message>(index, id)`; the three UiState color getters → `scanlateit_ui::color::rgba_to_color` (imported). Import pruning: dropped `Range`, `Duration`/`Vector`/`Rectangle`-related widget-op imports (`widget_op`, `Operation`, `Outcome`, `Scrollable`, `operate`, `WidgetId`, `AbsoluteOffset`), `decode_page`, `MAX_DECODE_EDGE`, `THUMB_DECODE_EDGE`, `PANEL_LIST_ID`/`panel_row_id`; `Duration` stays (the `OcrTick` subscription uses it), `tokio` stays (inpaint/style spawn_blocking).
7. **Line counts (now):** `src/app.rs` **1968** (was 2155; net -187 this session), `ui/decode.rs` 358 (+251), `ui/results.rs` 410 (+81), `ui/color.rs` 29 (new).
8. **Verification:** `cargo check` clean (only pre-existing warnings: ui overlay dead code, unused `FetchModels` variant, unused `field`); `cargo test -p scanlateit-ui` 19 pass; `cargo test -p scanlateit` 23 pass; `cargo test --workspace` — root + 7 crates all pass (229 tests: 37 neverlie lib + 23 root + 18 inpaint + 32 model + 51 ocr + 15 styling + 34 translation + 19 ui), only the pre-existing neverlie doctest noise (13) + its standalone lib build failure.

**Handoff note:** nothing outstanding; session 6 (`src/app.rs` final pass) can start from green. Smoke (scroll fast → thumb-only, stop → settle swaps full-res, scroll far → full caches evicted, panel-row click of a far page centers + full-res after debounce, panel scrolls to a selected overlay entry) stays for Session 7's full manual pass.

### 5a. `ui/Cargo.toml`

Add `tokio = { version = "1", features = ["rt", "time"] }` (iced already pulls tokio into the graph; this makes it a direct dep for `spawn_blocking` + `time::sleep`).

### 5b. `ui/src/main_area/decode.rs` additions

Move the consts from app.rs: `DECODE_PRELOAD: usize = 2`, `SETTLE_DEBOUNCE: Duration = 150ms`, `FULL_KEEP_MARGIN: usize = 4` (module-level `pub(crate)` or `pub`).

```rust
/// The settled-viewport decode scheduler: debounces visible-range changes
/// (`TilesVisible`), backs the settled window with full-res decodes and
/// evicts far-away full caches. The app owns one of these and forwards the
/// visible-range / settle-elapsed / scroll-ended / decode-finished messages.
#[derive(Debug, Default)]
pub struct Scheduler {
    settle_seq: u64,
    pending_settle: Option<(u64, Range<usize>)>,
    settled: Option<Range<usize>>,
}

impl Scheduler {
    pub fn new() -> Self
    /// The range a settled viewport gets backed with full decodes: the
    /// visible range plus [`DECODE_PRELOAD`] pages on each side.
    pub fn full_window(len: usize, range: &Range<usize>) -> Range<usize>
    /// Bumps the settle generation and spawns the debounce task; `map`
    /// turns the sequence number into the app's message.
    pub fn schedule<T>(&mut self, range: Range<usize>, map: impl Fn(u64) -> T + Send + Clone + 'static) -> iced::Task<T>
    /// True when `seq` is the pending generation (stale debounces no-op).
    pub fn accept_elapsed(&mut self, seq: u64) -> bool
    /// The settled range, if any.
    pub fn settled(&self) -> Option<&Range<usize>>
    /// Whether `index` lies outside the settled window (needs a settle).
    pub fn needs_settle(&self, index: usize, len: usize) -> bool
    /// Whether a full decode for `index` should be kept (inside the settled
    /// window + preload) — used by the `FullDecoded` handler.
    pub fn keep_full(&self, len: usize, index: usize) -> bool
    /// Spawns full-res decodes for the pending settle window (visible pages
    /// first, then preload pages outward) and evicts far-away full caches.
    /// No-op when no settle is pending. `map` turns `(index, result)` into
    /// the app's message.
    pub fn settle<T>(
        &mut self,
        images: &mut [LoadedImage],
        map: impl Fn(usize, Result<Arc<DecodedPage>, String>) -> T + Send + Clone + 'static,
    ) -> iced::Task<T>
    /// Spawns thumb decodes for every undecoded image (used on image load).
    pub fn decode_thumbs<T>(
        &mut self,
        images: &mut [LoadedImage],
        map: impl Fn(usize, Result<Arc<DecodedPage>, String>) -> T + Send + Clone + 'static,
    ) -> iced::Task<T>
}
```

Port **verbatim** from app.rs: `full_window` (1541-1544), `decode_async` (1548-1552 — becomes a private fn in this module using `tokio::task::spawn_blocking` + `decode_page`), `schedule_settle` (1558-1566 → `schedule`), `settle_full` (1643-1687 → `settle`, with `Task::batch` of per-page `Task::perform`s mapped through the `map` closure; the keep-range uses `FULL_KEEP_MARGIN`), the thumb batch (app.rs:1753-1766 → `decode_thumbs`).

Borrow note for `settle`/`decode_thumbs`: take `&mut [LoadedImage]`, collect the (index, path) work list and mark `Tier::Decoding` first, then drop the borrow before building tasks.

### 5c. `ui/src/panel/results.rs`

```rust
/// Scrolls the results list so the row of `(index, id)` is fully visible
/// (centered when out of view); no-op when already visible. Generic over the
/// message type so the app can return it directly.
pub fn scroll_to_row<T>(index: usize, id: EntryId) -> iced::Task<T>
```

Move `MeasurePanelRow` (app.rs:1573-1626) here verbatim, but generic: `impl<T> Operation<T> for MeasurePanelRow`, `finish(&self) -> Outcome<T>`, and build via `iced::advanced::widget::operate(...)` (a `Task::widget`, requires `T: Send + 'static` — the app's `Message` satisfies it). Uses the already-public `PANEL_LIST_ID`/`panel_row_id` in this module. Add the needed imports (`iced::advanced::widget::operation::{self as widget_op, Operation, Outcome, Scrollable}`, `iced::advanced::widget::{operate, Id as WidgetId}`, `iced::widget::operation::AbsoluteOffset`, `iced::Vector`).

### 5d. New `ui/src/color.rs`

```rust
/// Converts an RGBA byte color to an iced [`Color`].
pub fn rgba_to_color(rgba: [u8; 4]) -> Color
```

Re-export from `ui/src/lib.rs`: `pub mod color;`.

### 5e. Tests (ui crate)

- `decode.rs`: pure-math tests for `full_window` (clamps at both ends), `needs_settle`, `keep_full`, `accept_elapsed` (stale seq rejected, pending taken on accept), `settled`. (Task-building paths are glue; don't unit-test iced tasks.)
- `color.rs`: `rgba_to_color` maps channels + alpha 255 → 1.0.

### 5f. Rewire `src/app.rs`

- Fields (233-238): replace `settle_seq: u64`, `pending_settle: Option<(u64, Range<usize>)>`, `settled: Option<Range<usize>>` with `scheduler: Scheduler` (import from `scanlateit_ui::main_area::decode::Scheduler`). Delete consts `DECODE_PRELOAD`, `SETTLE_DEBOUNCE`, `FULL_KEEP_MARGIN`.
- Delete `full_window`, `decode_async`, `schedule_settle`, `settle_full` (1541-1687).
- Handlers:
  - `TilesVisible` (2028) → `app.scheduler.schedule(range, move |seq| Message::SettleElapsed(seq))`.
  - `SettleElapsed(seq)` (2029-2037) → `if app.scheduler.accept_elapsed(seq) { app.scheduler.settle(&mut app.images, |i, r| Message::FullDecoded(i, r)) } else { Task::none() }`.
  - `TileScrollEnded` (2038) → `app.scheduler.settle(&mut app.images, |i, r| Message::FullDecoded(i, r))`.
  - `FullDecoded` (2039-2054) → keep the Tier assignment, but `keep` = `app.scheduler.keep_full(app.images.len(), index)`; eviction stays inside `settle`.
  - `ThumbDecoded` (2055-2063) unchanged.
  - `ImagesPicked` (1736-1772) → replace the thumb task batch with `app.scheduler.decode_thumbs(&mut app.images, |i, r| Message::ThumbDecoded(i, r))`.
  - `select_entry` (404-413) → `app.scheduler.needs_settle(index, app.images.len())` and `app.scheduler.schedule(index..index + 1, ...)`.
- `panel_scroll_task` (1630-1639) → `scanlateit_ui::panel::results::scroll_to_row::<Message>(index, id)` (delete local fn + struct; the `EntryClicked` and `start_inline_edit` call sites change to the ui call).
- `rgba_to_color` (474-476) → `scanlateit_ui::color::rgba_to_color` (update the three `UiState` impl call sites).
- Prune now-unused imports (`Range` still used by Message? `TilesVisible(Range<usize>)` lives in ui/event.rs — app.rs may still need `Range` for scheduler calls; check).

### 5g. Verification

```powershell
cargo check
cargo test -p scanlateit-ui
cargo test -p scanlateit
```

Smoke: scroll fast (thumb-only, no full-res storm), stop → settle swaps in full-res near the viewport, scroll far away → full caches evicted (task manager memory drops), click a panel row of a far page → main area centers + full-res after debounce, panel scrolls to a selected overlay entry.

**Handoff note:** if time runs out, commit `Scheduler` + tests first (self-contained); the app rewiring may spill into the next session — leave the handler list in this section marked done/remaining.

---

# Session 6 — `src/app.rs` final pass

**Goal:** remove every trace of the moved code; the file is only the channel + glue.

1. Grep app.rs for: `assemble_run`, `start_ocr_stream` internals, `build_canvas`, `CanvasBuild`, `OcrRunOutcome`, `sync_translate`, `schedule_settle`, `settle_full`, `MeasurePanelRow`, `panel_scroll_task`, `quad_intersects_rect`, `rgba_to_color`, `file_name`, `INITIAL_PRESET_SLOTS`, `style_presets` (old field name), `settle_seq`, `pending_settle`, `styled`, `styling_pending`. Everything must be gone or renamed.
2. Prune the `use` block to only what `update`/`view`/`UiState`/`boot` still need. Watch for: `VecDeque`, `Range`, `Rectangle`, `Vector`, `WidgetId`, `operate`, `widget_op`, `AbsoluteOffset`, `panel_row_id`/`PANEL_LIST_ID`, decode imports (`decode_page`, `DecodedPage`, `PageDecode`, `Tier`, `MAX_DECODE_EDGE`, `THUMB_DECODE_EDGE`).
3. Review the App struct field docs — each remaining field should still be accurate; drop stale doc comments referencing deleted fields.
4. Decide the final shape of the OCR bookkeeping fields: `ocr_plans`/`ocr_dims` are still needed by the `OcrStreamRun` handler (assembly reads them). Keep. `pending`, `ocr_runs`, `ocr_total`, `ocr_failed`, `ocr_cancelled` stay (status + finalize).
5. Run `cargo clippy --workspace` if available and fix only what the move introduced (unused imports, needless returns). **Do not** reformat unrelated code.
6. Line count check: app.rs should be ~1200-1400 lines (excluding nothing — tests included).

**Handoff note:** if leftover references exist, list them here explicitly for the next session.

---

# Session 7 — Full verification + commit

1. `cargo check` (clean), `cargo test --workspace`.
2. `cargo clippy --workspace -- -D warnings` (if the project was clippy-clean before; otherwise only the new code must be clean).
3. Full manual smoke in `scanlateit.exe` or `cargo run`:
   - Boot with a saved `settings.json` (provider restored, model list fetched).
   - Open 10+ images incl. one very tall page and one corrupt file.
   - OCR: bubbles whole across page seams, no duplicates, cancel mid-run, fallback path on the corrupt page.
   - Auto style-detect: every entry classified once; manual re-run works.
   - Inpaint a range with and without OCR boxes inside.
   - Translate a batch; profile `english(auto)` created; switch profiles back to Default — OCR text untouched.
   - Scroll fast/slow; settle; panel↔main-area reveal both ways; inline edit from overlay and panel; presets add/replace/remove/apply.
4. `git log` review of session commits; ensure each session has its own commit (or at least the tree is at the final state).
5. Update this file's header target notes if the final line count differs.

---

# Appendix A — Full move inventory (app.rs → crate)

| app.rs code | app.rs lines (as audited) | Moves to | Becomes |
|---|---|---|---|
| `OcrRunOutcome` | 111-128 | ocr/session.rs | `RunEvent` (with `index` field) |
| `file_name` | 341-345 | translation | `file_tag` |
| `sync_translate_providers` | 419-436 | translation/session.rs | `Session::sync` |
| `sync_translate_models` | 443-471 | translation/session.rs | `Session::sync_models` |
| `rgba_to_color` | 474-476 | ui/color.rs | `rgba_to_color` |
| preset seeding block | 312-328 | model/style.rs | `StylePresets::default_presets` |
| `start_ocr_stream` | 1057-1175 | ocr/session.rs | `RunSession::step` pump |
| `CanvasBuild`/`build_canvas` | 1178-1238 | ocr/session.rs | `BuiltCanvas`/`build_canvas` |
| `dispatch_run` | 1244-1288 | ocr/session.rs | inlined into `step` |
| `quad_intersects_rect` | 1292-1295 | model/entry.rs | `Quad::intersects_rect` |
| `classify_entries`/`start_style_jobs` | 1326-1391 | styling | `JobTracker` + `Engine::classify_entry` (enumeration stays app-side) |
| `assemble_run` | 1400-1467 | ocr/lib.rs | `assemble` |
| `commit_run_result` | 1471-1479 | ocr/lib.rs | `RunResult::commit_entries` |
| `flush_held_boundary` | 1484-1492 | ocr/lib.rs | `BoundaryState::commit` |
| `full_window`/`decode_async`/`schedule_settle`/`settle_full` | 1541-1687 | ui/main_area/decode.rs | `Scheduler` |
| `MeasurePanelRow`/`panel_scroll_task` | 1573-1639 | ui/panel/results.rs | `scroll_to_row<T>` |
| connect modal validation | 2206-2228 | translation | `validate_connection` |
| `translate_*` fields + handlers | 189-208, 2064-2261 | translation/session.rs | `Session` |
| `styled`/`styling_engine`/`styling_pending` | 161-170 | styling/tracker.rs | `JobTracker` |
| `settle_seq`/`pending_settle`/`settled` | 233-238 | ui/main_area/decode.rs | `Scheduler` fields |
| fork-on-edit block | 2386-2403 | model/profile.rs | `Profiles::fork_for_edit` |
| preset handlers (Add/Replace/Remove) | 2474-2493 | model/style.rs | `StylePresets::add/replace/remove` |
| translate store loop | 2557-2575 | model/project.rs | `Project::store_translation` |

**Stays in app.rs:** `Message` + `From<UiEvent>`, `update`, `view`, `subscription`, `boot`, `UiState` impl, `App` shell fields (images, engines, `cancel`, `ocr_workers`, `ocr_plans`/`ocr_dims`, OCR counters, `pending_inpaint`, selection/editing/style-working fields, `panes`, toggles), `maybe_start_ocr`, `finalize_run`, `start_inline_edit`/`clear_editing`/`seed_style_inputs`/`select_entry` (thin glue), status strings, and the channel-level unit tests.

# Appendix B — Test move map

| app.rs test | destination |
|---|---|
| `seeded_presets_cover_the_expected_variants` | model/style.rs (against `StylePresets::default_presets`) |
| `add_preset_*`, `replace_preset_*`, `remove_preset_*` | model/style.rs rule tests (app.rs channel copies stay) |
| `first/later/edits_on_non_original/panel_edit_forks` fork rules | model/profile.rs `fork_for_edit` tests (channel copies stay) |
| `default_style_round_trips_all_fields` | model/style.rs |
| preset/fork/geometry rule tests (new) | model |
| `file_tag`, `validate_connection`, `Session` (new) | translation |
| `assemble`, `commit_entries`, `BoundaryState::commit` (new) | ocr |
| `JobTracker` (new) | styling |
| `Scheduler` math, `rgba_to_color` (new) | ui |
| double-click / toolbar / entry-move / edit-submit flows | stay in app.rs (channel tests) |
