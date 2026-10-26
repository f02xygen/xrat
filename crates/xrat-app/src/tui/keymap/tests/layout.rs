use crossterm::event::{KeyCode, KeyModifiers};

use crate::tui::app::{TestScope, TuiAction, TuiPanel, TuiView};
use crate::tui::keymap::{TuiKeyEvent, action_for_key_with_import};

fn key(code: KeyCode) -> TuiKeyEvent {
    TuiKeyEvent::new(code, KeyModifiers::empty())
}

fn ctrl_key(ch: char) -> TuiKeyEvent {
    TuiKeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
}

#[test]
fn layout_independent_navigation_via_base_layout_key() {
    let mut pending = None;

    // Russian 'о' (physical key 'j' on QWERTY):
    let ev = key(KeyCode::Char('о')).with_base_layout('j');
    let action = action_for_key_with_import(
        ev,
        TuiView::Configs,
        TuiPanel::Table,
        &mut pending,
        false,
        false,
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(action, TuiAction::MoveDown);

    // Russian 'л' (physical key 'k' on QWERTY):
    let ev = key(KeyCode::Char('л')).with_base_layout('k');
    let action = action_for_key_with_import(
        ev,
        TuiView::Configs,
        TuiPanel::Table,
        &mut pending,
        false,
        false,
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(action, TuiAction::MoveUp);

    // French AZERTY 'a' (physical key 'q' on QWERTY):
    let ev = key(KeyCode::Char('a')).with_base_layout('q');
    let action = action_for_key_with_import(
        ev,
        TuiView::Configs,
        TuiPanel::Table,
        &mut pending,
        false,
        false,
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(action, TuiAction::Quit);

    // German QWERTZ 'z' (physical key 'y' on QWERTY):
    let ev = key(KeyCode::Char('z')).with_base_layout('y');
    let action = action_for_key_with_import(
        ev,
        TuiView::Configs,
        TuiPanel::Table,
        &mut pending,
        false,
        false,
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(action, TuiAction::OpenQrFocused);
}

#[test]
fn layout_independent_ctrl_shortcuts_and_text_input() {
    let mut pending = None;

    // Russian 'с' with Ctrl (physical key 'c'):
    let ev = ctrl_key('с').with_base_layout('c');
    let action = action_for_key_with_import(
        ev,
        TuiView::Configs,
        TuiPanel::Table,
        &mut pending,
        false,
        false,
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(action, TuiAction::Quit);

    // In text input (search): typed non-Latin character is preserved for entering text
    let ev = key(KeyCode::Char('щ')).with_base_layout('o');
    let action = action_for_key_with_import(
        ev,
        TuiView::Configs,
        TuiPanel::Table,
        &mut pending,
        false,
        true, // editing_search
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(action, TuiAction::SearchInput('щ'));

    // In text input (search): Ctrl+ц (physical key 'w') normalizes to Ctrl+W (delete word)
    let ev = ctrl_key('ц').with_base_layout('w');
    let action = action_for_key_with_import(
        ev,
        TuiView::Configs,
        TuiPanel::Table,
        &mut pending,
        false,
        true, // editing_search
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(action, TuiAction::SearchDeleteWord);

    // In text input (search): Ctrl+г (physical key 'u') normalizes to Ctrl+U (clear input)
    let ev = ctrl_key('г').with_base_layout('u');
    let action = action_for_key_with_import(
        ev,
        TuiView::Configs,
        TuiPanel::Table,
        &mut pending,
        false,
        true, // editing_search
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(action, TuiAction::ClearSearch);
}

#[test]
fn layout_independent_chords_work() {
    let mut pending = None;

    // Persian/Arabic or Russian physical 't' key:
    let ev1 = key(KeyCode::Char('е')).with_base_layout('t');
    let armed = action_for_key_with_import(
        ev1,
        TuiView::Configs,
        TuiPanel::Table,
        &mut pending,
        false,
        false,
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(armed, TuiAction::None);
    assert_eq!(pending, Some('t'));

    let ev2 = key(KeyCode::Char('е')).with_base_layout('t');
    let resolved = action_for_key_with_import(
        ev2,
        TuiView::Configs,
        TuiPanel::Table,
        &mut pending,
        false,
        false,
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(resolved, TuiAction::StartTest(TestScope::Focused));
    assert_eq!(pending, None);
}

#[test]
fn qwertty_kitty_sequence_conversion_test() {
    use qwertty::SemanticDecoder;

    let mut decoder = SemanticDecoder::new();

    // Kitty escape sequence for Russian 'щ' at physical key 'o' (codepoints 1097::111):
    let events = decoder.feed(b"\x1b[1097::111;1u");
    assert_eq!(events.len(), 1);
    let qwertty_key = match &events[0] {
        qwertty::Event::Key(k) => k,
        other => panic!("expected Key event, got {:?}", other),
    };

    let tui_event = TuiKeyEvent::from(qwertty_key);
    assert_eq!(tui_event.code, KeyCode::Char('щ'));
    assert_eq!(tui_event.base_layout_key, Some('o'));

    // In navigation mode it normalizes to 'o':
    let normalized = tui_event.normalize(false);
    assert_eq!(normalized.code, KeyCode::Char('o'));

    // In text input mode it preserves 'щ':
    let text_norm = tui_event.normalize(true);
    assert_eq!(text_norm.code, KeyCode::Char('щ'));
}
