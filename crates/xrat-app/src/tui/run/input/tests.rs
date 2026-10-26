use super::*;
use crate::tui::app::{ImportModalState, ImportModalStep};
use crate::tui::data::TuiData;

fn text_key() -> TuiKeyEvent {
    let mut decoder = qwertty::SemanticDecoder::new();
    let events = decoder.feed(b"\x1b[101;1;101:769u");
    match &events[0] {
        qwertty::Event::Key(key) => TuiKeyEvent::from(key),
        other => panic!("{other:?}"),
    }
}

#[test]
fn associated_text_preserves_all_codepoints_in_search_and_import() {
    for modifiers in [
        KeyModifiers::empty(),
        KeyModifiers::ALT,
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ] {
        let mut app = TuiApp::with_data(TuiData::default());
        let mut key = text_key();
        key.modifiers = modifiers;
        app.apply(TuiAction::BeginSearch);
        assert!(apply_key_text(&mut app, &key));
        assert_eq!(app.config_list.search_query, "e\u{301}");
        app.import_modal = Some(ImportModalState {
            step: ImportModalStep::Link,
            input: String::new(),
            error: None,
        });
        assert!(apply_key_text(&mut app, &key));
        assert_eq!(app.import_modal.as_ref().unwrap().input, "e\u{301}");
    }
}

#[test]
fn associated_text_does_not_bypass_shortcuts_or_navigation() {
    let mut app = TuiApp::with_data(TuiData::default());
    let mut key = text_key();
    assert!(!apply_key_text(&mut app, &key));
    app.apply(TuiAction::BeginSearch);
    key.modifiers = KeyModifiers::CONTROL;
    assert!(!apply_key_text(&mut app, &key));
    assert!(app.config_list.search_query.is_empty());
}
