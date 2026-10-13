//! 前缀解析与目标范围。

use cping_core::target::{build_targets, parse_prefix, Ipv4Prefix, TargetRange};
use cping_core::CoreError;
use std::net::Ipv4Addr;

#[test]
fn parses_plain_prefix() {
    let p = parse_prefix("192.168.1").expect("应解析成功");
    assert_eq!(p, Ipv4Prefix::new(192, 168, 1));
    assert_eq!(p.to_string(), "192.168.1");
    assert_eq!(p.with_host(7), Ipv4Addr::new(192, 168, 1, 7));
}

#[test]
fn trims_surrounding_whitespace() {
    assert_eq!(
        parse_prefix("  10.0.0  ").unwrap(),
        Ipv4Prefix::new(10, 0, 0)
    );
}

#[test]
fn accepts_boundary_octets() {
    assert_eq!(parse_prefix("0.0.0").unwrap(), Ipv4Prefix::new(0, 0, 0));
    assert_eq!(
        parse_prefix("255.255.255").unwrap(),
        Ipv4Prefix::new(255, 255, 255)
    );
}

#[test]
fn rejects_empty_and_blank() {
    for bad in ["", "   "] {
        assert!(matches!(
            parse_prefix(bad),
            Err(CoreError::InvalidPrefix(_))
        ));
    }
}

#[test]
fn rejects_wrong_segment_count() {
    for bad in ["192.168", "192.168.1.1", "192", "192.168.1."] {
        let err = parse_prefix(bad).unwrap_err();
        assert!(
            matches!(err, CoreError::InvalidPrefix(_)),
            "{bad} 应该被拒绝，实际 {err:?}"
        );
    }
}

#[test]
fn rejects_non_numeric() {
    for bad in [
        "a.b.c",
        "192.168.x",
        "192.16 8.1",
        "192.168.1a",
        "1 92.168.1",
    ] {
        assert!(
            matches!(parse_prefix(bad), Err(CoreError::InvalidPrefix(_))),
            "{bad} 应该被拒绝"
        );
    }
}

#[test]
fn rejects_out_of_range() {
    for bad in ["256.1.1", "1.256.1", "1.1.256", "999.999.999"] {
        assert!(
            matches!(parse_prefix(bad), Err(CoreError::InvalidPrefix(_))),
            "{bad} 应该被拒绝"
        );
    }
}

#[test]
fn rejects_leading_zeros() {
    // 设计决定：拒绝前导零，避免 "01.2.3" 这类语义歧义
    for bad in ["01.2.3", "1.02.3", "1.2.003"] {
        assert!(
            matches!(parse_prefix(bad), Err(CoreError::InvalidPrefix(_))),
            "{bad} 应该被拒绝"
        );
    }
    // 单独的 "0" 是合法的
    assert!(parse_prefix("0.0.0").is_ok());
}

#[test]
fn rejects_signed_and_negative() {
    for bad in ["+1.2.3", "-1.2.3", "1.-2.3"] {
        assert!(
            matches!(parse_prefix(bad), Err(CoreError::InvalidPrefix(_))),
            "{bad} 应该被拒绝"
        );
    }
}

#[test]
fn standard_range_is_255_from_1_to_255() {
    let p = parse_prefix("192.168.1").unwrap();
    let targets = build_targets(&p, TargetRange::Standard).unwrap();
    assert_eq!(targets.len(), 255, "Standard 应为 255 个目标");
    assert_eq!(targets.first().unwrap().octets()[3], 1);
    assert_eq!(targets.last().unwrap().octets()[3], 255);
    assert!(
        !targets.contains(&Ipv4Addr::new(192, 168, 1, 0)),
        "Standard 不得包含 .0"
    );
}

#[test]
fn full_range_is_256_including_zero() {
    let p = parse_prefix("10.0.0").unwrap();
    let targets = build_targets(&p, TargetRange::Full).unwrap();
    assert_eq!(targets.len(), 256);
    assert_eq!(targets.first().unwrap().octets()[3], 0);
    assert_eq!(targets.last().unwrap().octets()[3], 255);
}

#[test]
fn hosts_only_excludes_0_and_255() {
    let p = parse_prefix("10.0.0").unwrap();
    let targets = build_targets(&p, TargetRange::HostsOnly).unwrap();
    assert_eq!(targets.len(), 254);
    assert_eq!(targets.first().unwrap().octets()[3], 1);
    assert_eq!(targets.last().unwrap().octets()[3], 254);
}

#[test]
fn targets_are_ascending_and_unique() {
    let p = parse_prefix("172.16.5").unwrap();
    let targets = build_targets(&p, TargetRange::Full).unwrap();
    for w in targets.windows(2) {
        assert!(w[0].octets()[3] < w[1].octets()[3]);
    }
    let mut dedup = targets.clone();
    dedup.dedup();
    assert_eq!(dedup.len(), targets.len(), "目标必须唯一");
}

#[test]
fn range_counts_match_contract() {
    assert_eq!(TargetRange::Standard.host_count(), 255);
    assert_eq!(TargetRange::Full.host_count(), 256);
    assert_eq!(TargetRange::HostsOnly.host_count(), 254);
    assert_eq!(TargetRange::default(), TargetRange::Standard);
}
