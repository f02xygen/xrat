use super::*;
use crate::app::terminal::output;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FailureStage {
    Icmp,
    Tcp,
    RealDelay,
    Download,
    Upload,
}

#[derive(Default)]
pub(crate) struct FailureSelection {
    stage: Option<FailureStage>,
}

pub(crate) fn merge_failure(
    result: &mut TestResult,
    selection: &mut FailureSelection,
    stage: FailureStage,
    failure_kind: Option<FailureKind>,
    failure_reason: Option<String>,
) {
    if selection.stage.is_some_and(|selected| selected > stage) {
        return;
    }
    selection.stage = Some(stage);
    result.failure_kind = failure_kind;
    result.failure_reason = failure_reason;
}

pub(crate) fn print_download_result(
    success: bool,
    mbps: Option<f64>,
    failure_reason: Option<&str>,
) {
    if success {
        println!(
            "{}",
            output::success(
                format!("{:.2} Mbps", mbps.unwrap_or_default()),
                output::color_enabled()
            )
        );
    } else {
        println!("FAIL {}", failure_reason.unwrap_or("failed"));
    }
}

pub(crate) fn print_stage_result(
    success: bool,
    latency_ms: Option<u32>,
    failure_reason: Option<&str>,
) {
    if success {
        println!(
            "{}",
            output::success(
                format!("{}ms", latency_ms.unwrap_or_default()),
                output::color_enabled()
            )
        );
    } else {
        println!("FAIL {}", failure_reason.unwrap_or("failed"));
    }
}

#[cfg(test)]
mod tests;
