//! 分类边界。必须明确覆盖 49 / 50 / 99 / 100 / 101，以及超时、不可达、权限错误。

use cping_core::classify::{classify_rtt, classify_status, LatencyClass, ResultClass, Thresholds};
use cping_core::result::ProbeStatus;
use cping_core::CoreError;

#[test]
fn default_thresholds_match_original_semantics() {
    let t = Thresholds::default();
    assert_eq!(t.low_max_ms, 50);
    assert_eq!(t.mid_max_ms, 100);
    t.validate().expect("默认阈值必须合法");
}

#[test]
fn boundary_values_are_exact() {
    let t = Thresholds::default();
    // 49 → 低；50 → 中（原版文案「50ms~100ms 之间」含 50）
    assert_eq!(classify_rtt(49, &t), LatencyClass::Low);
    assert_eq!(classify_rtt(50, &t), LatencyClass::Mid);
    // 100 → 中（含 100）；101 → 高
    assert_eq!(classify_rtt(99, &t), LatencyClass::Mid);
    assert_eq!(classify_rtt(100, &t), LatencyClass::Mid);
    assert_eq!(classify_rtt(101, &t), LatencyClass::High);
}

#[test]
fn extreme_rtts_are_handled() {
    let t = Thresholds::default();
    assert_eq!(classify_rtt(0, &t), LatencyClass::Low);
    assert_eq!(classify_rtt(1, &t), LatencyClass::Low);
    assert_eq!(classify_rtt(u32::MAX, &t), LatencyClass::High);
}

#[test]
fn custom_thresholds_shift_boundaries() {
    let t = Thresholds::new(10, 20);
    assert_eq!(classify_rtt(9, &t), LatencyClass::Low);
    assert_eq!(classify_rtt(10, &t), LatencyClass::Mid);
    assert_eq!(classify_rtt(20, &t), LatencyClass::Mid);
    assert_eq!(classify_rtt(21, &t), LatencyClass::High);
}

#[test]
fn invalid_thresholds_are_rejected() {
    assert!(matches!(
        Thresholds::new(0, 100).validate(),
        Err(CoreError::InvalidConfig(_))
    ));
    assert!(matches!(
        Thresholds::new(100, 100).validate(),
        Err(CoreError::InvalidConfig(_))
    ));
    assert!(matches!(
        Thresholds::new(100, 50).validate(),
        Err(CoreError::InvalidConfig(_))
    ));
}

#[test]
fn status_maps_to_expected_class() {
    let t = Thresholds::default();
    let cases = [
        (ProbeStatus::Online { rtt_ms: 12 }, Some(ResultClass::Low)),
        (ProbeStatus::Online { rtt_ms: 75 }, Some(ResultClass::Mid)),
        (ProbeStatus::Online { rtt_ms: 250 }, Some(ResultClass::High)),
        (ProbeStatus::Timeout, Some(ResultClass::Offline)),
        (ProbeStatus::Unreachable, Some(ResultClass::Offline)),
        (ProbeStatus::PermissionDenied, Some(ResultClass::Error)),
        (ProbeStatus::Unsupported, Some(ResultClass::Error)),
        (ProbeStatus::Error("boom".into()), Some(ResultClass::Error)),
        (ProbeStatus::Pending, None),
    ];
    for (status, expected) in cases {
        assert_eq!(classify_status(&status, &t), expected, "状态 {status:?}");
    }
}

#[test]
fn error_class_is_distinct_from_offline() {
    let t = Thresholds::default();
    // 明确要求：权限错误不得被当作「无回应」
    assert_ne!(
        classify_status(&ProbeStatus::PermissionDenied, &t),
        classify_status(&ProbeStatus::Timeout, &t)
    );
    assert_ne!(ResultClass::Error, ResultClass::Offline);
    assert_ne!(ResultClass::Error.key(), ResultClass::Offline.key());
}

#[test]
fn online_detection_is_strict() {
    assert!(ProbeStatus::Online { rtt_ms: 0 }.is_online());
    assert!(!ProbeStatus::Timeout.is_online());
    assert!(!ProbeStatus::Unreachable.is_online());
    assert!(!ProbeStatus::PermissionDenied.is_online());
    assert!(!ProbeStatus::Pending.is_online());
}

#[test]
fn pending_is_not_final() {
    assert!(!ProbeStatus::Pending.is_final());
    for s in [
        ProbeStatus::Online { rtt_ms: 1 },
        ProbeStatus::Timeout,
        ProbeStatus::Unreachable,
        ProbeStatus::PermissionDenied,
        ProbeStatus::Unsupported,
        ProbeStatus::Error("x".into()),
    ] {
        assert!(s.is_final(), "{s:?} 应为终态");
    }
}
