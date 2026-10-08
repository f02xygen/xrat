use super::*;

#[test]
fn proxy_failure_survives_later_icmp_success_or_failure() {
    for icmp_failure in [false, true] {
        let mut result = TestResult::default();
        let mut selection = FailureSelection::default();
        merge_failure(
            &mut result,
            &mut selection,
            FailureStage::RealDelay,
            Some(FailureKind::Tls),
            Some("TLS handshake failed".into()),
        );
        merge_failure(
            &mut result,
            &mut selection,
            FailureStage::Icmp,
            icmp_failure.then_some(FailureKind::Unknown),
            icmp_failure.then(|| "Ping failed".into()),
        );
        assert_eq!(result.failure_kind, Some(FailureKind::Tls));
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("TLS handshake failed")
        );
        assert_eq!(
            overall_status(&result, true, false, true, false, false),
            TestStatus::Failed
        );
    }
}

#[test]
fn successful_proxy_probe_supersedes_icmp_failure_in_either_order() {
    for icmp_first in [false, true] {
        let mut result = TestResult {
            real_delay_ok: true,
            ..Default::default()
        };
        let mut selection = FailureSelection::default();
        let stages = if icmp_first {
            [FailureStage::Icmp, FailureStage::RealDelay]
        } else {
            [FailureStage::RealDelay, FailureStage::Icmp]
        };
        for stage in stages {
            let failed = stage == FailureStage::Icmp;
            merge_failure(
                &mut result,
                &mut selection,
                stage,
                failed.then_some(FailureKind::Unknown),
                failed.then(|| "Ping failed".into()),
            );
        }
        assert_eq!(result.failure_kind, None);
        assert_eq!(result.failure_reason, None);
        assert_eq!(
            overall_status(&result, true, false, true, false, false),
            TestStatus::Ok
        );
    }
}

#[test]
fn failure_selection_matches_status_priority_for_every_stage_pair() {
    let stages = [
        FailureStage::Icmp,
        FailureStage::Tcp,
        FailureStage::RealDelay,
        FailureStage::Download,
        FailureStage::Upload,
    ];
    for higher in 1..stages.len() {
        for lower in 0..higher {
            for higher_first in [false, true] {
                for higher_succeeds in [false, true] {
                    let mut result = TestResult::default();
                    let mut selection = FailureSelection::default();
                    let order = if higher_first {
                        [stages[higher], stages[lower]]
                    } else {
                        [stages[lower], stages[higher]]
                    };
                    for stage in order {
                        let failed = stage != stages[higher] || !higher_succeeds;
                        merge_failure(
                            &mut result,
                            &mut selection,
                            stage,
                            failed.then_some(FailureKind::Timeout),
                            failed.then(|| {
                                if stage == stages[higher] {
                                    "authoritative failure"
                                } else {
                                    "lower failure"
                                }
                                .into()
                            }),
                        );
                    }
                    result.icmp_ok = higher_succeeds;
                    result.tcp_ok = higher_succeeds;
                    result.real_delay_ok = higher_succeeds;
                    result.download_ok = higher_succeeds;
                    result.upload_ok = higher_succeeds;
                    let ran = |stage| stage == stages[higher] || stage == stages[lower];
                    let status = overall_status(
                        &result,
                        ran(FailureStage::Icmp),
                        ran(FailureStage::Tcp),
                        ran(FailureStage::RealDelay),
                        ran(FailureStage::Download),
                        ran(FailureStage::Upload),
                    );
                    assert_eq!(
                        status,
                        if higher_succeeds {
                            TestStatus::Ok
                        } else {
                            TestStatus::Failed
                        }
                    );
                    assert_eq!(
                        result.failure_reason.as_deref(),
                        if higher_succeeds {
                            None
                        } else {
                            Some("authoritative failure")
                        }
                    );
                }
            }
        }
    }
}
