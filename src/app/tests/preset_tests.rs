use crate::app::tests::app_with_entry;
use crate::app::{update, Message};
use scanlateit_model::{EntryId, EntryStyle, TextAlign, TextGradientDir};
use scanlateit_settings::INITIAL_PRESET_SLOTS;
use scanlateit_ui::event::UiEvent;

#[test]
fn applying_a_preset_seeds_working_style_and_entry() {
    let (mut app, id) = app_with_entry();
    app.selected = Some((0, id));
    app.style_working.bg_color = [1, 2, 3, 255];
    let _ = update(&mut app, Message::Ui(UiEvent::StylePresetApply(1)));
    let preset = app.presets.get(1).expect("preset 1 seeded");
    assert_eq!(app.style_working, preset);
    assert_eq!(app.project.entry_style(id), preset);
    assert_eq!(app.style_bg_radius, preset.bg_radius.to_string());
}

#[test]
fn applying_a_preset_without_selection_or_out_of_range_is_a_noop() {
    let (mut app, _id) = app_with_entry();
    app.style_working.bg_color = [1, 2, 3, 255];
    app.selected = None;
    let _ = update(&mut app, Message::Ui(UiEvent::StylePresetApply(0)));
    assert_eq!(app.style_working.bg_color, [1, 2, 3, 255]);
    let image_id = app.images[0].image_id;
    app.selected = Some((0, app.project.ocr.visible_for(image_id).next().unwrap().id));
    let _ = update(&mut app, Message::Ui(UiEvent::StylePresetApply(999)));
    assert_eq!(app.style_working.bg_color, [1, 2, 3, 255]);
}

#[test]
fn applying_an_empty_preset_slot_is_a_noop() {
    let (mut app, id) = app_with_entry();
    app.selected = Some((0, id));
    app.style_working.bg_color = [1, 2, 3, 255];
    app.project.set_entry_style(id, app.style_working.clone());
    let _ = update(&mut app, Message::Ui(UiEvent::StylePresetApply(5)));
    assert_eq!(app.style_working.bg_color, [1, 2, 3, 255]);
    assert_eq!(app.project.entry_style(id).bg_color, [1, 2, 3, 255]);
}

#[test]
fn add_preset_fills_the_first_empty_slot() {
    let (mut app, _id) = app_with_entry();
    app.style_working.bg_color = [9, 9, 9, 255];
    let _ = update(&mut app, Message::Ui(UiEvent::StylePresetAdd));
    let _ = update(&mut app, Message::Ui(UiEvent::StylePresetAdd));
    let _ = update(&mut app, Message::Ui(UiEvent::StylePresetAdd));
    assert_eq!(app.presets.len(), INITIAL_PRESET_SLOTS);
    assert_eq!(app.presets.get(5), Some(app.style_working.clone()));
    assert_eq!(app.presets.get(6), Some(app.style_working.clone()));
    assert_eq!(app.presets.get(7), Some(app.style_working.clone()));
}

#[test]
fn add_preset_appends_when_all_slots_are_full() {
    let (mut app, _id) = app_with_entry();
    for i in 0..INITIAL_PRESET_SLOTS {
        let mut style = EntryStyle::default();
        style.text_color = [i as u8, 0, 0, 255];
        app.presets.replace(i, style);
    }
    let _ = update(&mut app, Message::Ui(UiEvent::StylePresetAdd));
    assert_eq!(app.presets.len(), INITIAL_PRESET_SLOTS + 1);
    assert_eq!(app.presets.get(INITIAL_PRESET_SLOTS), Some(app.style_working.clone()));
}

#[test]
fn add_preset_refills_an_emptied_slot_before_appending() {
    let (mut app, _id) = app_with_entry();
    app.style_working.text_color = [7, 7, 7, 255];
    let _ = update(&mut app, Message::Ui(UiEvent::StylePresetRemove(2)));
    let _ = update(&mut app, Message::Ui(UiEvent::StylePresetAdd));
    assert_eq!(app.presets.len(), INITIAL_PRESET_SLOTS);
    assert_eq!(app.presets.get(2), Some(app.style_working.clone()));
}

#[test]
fn replace_preset_overwrites_filled_and_empty_slots() {
    let (mut app, _id) = app_with_entry();
    app.style_working.text_color = [42, 0, 0, 255];
    let _ = update(&mut app, Message::Ui(UiEvent::StylePresetReplace(1)));
    assert_eq!(app.presets.get(1), Some(app.style_working.clone()));
    let _ = update(&mut app, Message::Ui(UiEvent::StylePresetReplace(6)));
    assert_eq!(app.presets.get(6), Some(app.style_working.clone()));
    let _ = update(&mut app, Message::Ui(UiEvent::StylePresetReplace(999)));
    assert_eq!(app.presets.len(), INITIAL_PRESET_SLOTS);
}

#[test]
fn remove_preset_empties_the_slot() {
    let (mut app, _id) = app_with_entry();
    let _ = update(&mut app, Message::Ui(UiEvent::StylePresetRemove(0)));
    let _ = update(&mut app, Message::Ui(UiEvent::StylePresetRemove(999)));
    assert!(app.presets.get(0).is_none());
    assert_eq!(app.presets.len(), INITIAL_PRESET_SLOTS);
}

#[test]
fn style_font_sets_family_and_loads_font() {
    let (mut app, id) = app_with_entry();
    app.selected = Some((0, id));
    app.system_fonts
        .insert("Test".into(), "C:\\Windows\\Fonts\\arial.ttf".into());
    let _ = update(&mut app, Message::Ui(UiEvent::StyleFont("Test".to_string())));
    assert_eq!(app.style_working.font_family.as_deref(), Some("Test"));
    assert_eq!(
        app.project.entry_style(id).font_family.as_deref(),
        Some("Test")
    );
}

#[test]
fn style_text_align_sets_alignment() {
    let (mut app, id) = app_with_entry();
    app.selected = Some((0, id));
    let _ = update(&mut app, Message::Ui(UiEvent::StyleTextAlign(TextAlign::Right)));
    assert_eq!(app.style_working.text_align, TextAlign::Right);
    assert_eq!(
        app.project.entry_style(id).text_align,
        TextAlign::Right
    );
}

#[test]
fn style_gradient_dir_and_toggle_set_fields() {
    let (mut app, id) = app_with_entry();
    app.selected = Some((0, id));
    let _ = update(&mut app, Message::Ui(UiEvent::StyleGradientToggle(true)));
    assert!(app.style_working.text_gradient);
    assert!(app.project.entry_style(id).text_gradient);
    let _ = update(
        &mut app,
        Message::Ui(UiEvent::StyleGradientDir(TextGradientDir::LeftToRight)),
    );
    assert_eq!(app.style_working.gradient_dir, TextGradientDir::LeftToRight);
    assert_eq!(
        app.project.entry_style(id).gradient_dir,
        TextGradientDir::LeftToRight
    );
}
