use super::helpers::row;
use crate::tui::app::{TuiAction, TuiApp};
use crate::tui::data::TuiData;
use crate::tui::task::TuiTaskKind;

#[test]
fn cancel_test_batch_signals_running_token() {
    let mut app = TuiApp::with_data(TuiData::from_configs(vec![row(1)]));
    let (token, _receiver) = app.task_state.start(TuiTaskKind::TestBatch);

    app.apply(TuiAction::CancelTestBatch);

    assert!(token.is_cancelled());
}

#[test]
fn cancel_test_batch_without_running_task_is_a_noop() {
    let mut app = TuiApp::with_data(TuiData::from_configs(vec![row(1)]));

    app.apply(TuiAction::CancelTestBatch);

    assert!(app.task_state.cancellation.is_none());
}

#[test]
fn cancel_test_batch_clears_testing_config_ids() {
    let mut app = TuiApp::with_data(TuiData::from_configs(vec![row(1)]));
    let (_token, _receiver) = app.task_state.start(TuiTaskKind::TestBatch);
    app.testing_config_ids = vec![xrat_model::ConfigId(1)];

    app.apply(TuiAction::CancelTestBatch);

    assert!(app.testing_config_ids.is_empty());
}
