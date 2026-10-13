//! 16×16 矩阵索引。禁止“猜”索引关系，全部显式断言。

use cping_core::target::{
    build_targets, cell_index, cell_of_host, cell_of_ip, cell_position, matrix_slots, parse_prefix,
    TargetRange, MATRIX_CELLS, MATRIX_COLS, MATRIX_ROWS,
};
use std::net::Ipv4Addr;

#[test]
fn matrix_is_16_by_16() {
    assert_eq!(MATRIX_COLS, 16);
    assert_eq!(MATRIX_ROWS, 16);
    assert_eq!(MATRIX_CELLS, 256);
}

#[test]
fn index_and_position_are_inverse() {
    for index in 0..MATRIX_CELLS {
        let (row, col) = cell_position(index).expect("合法格号应有位置");
        assert!(row < MATRIX_ROWS && col < MATRIX_COLS);
        assert_eq!(cell_index(row, col), Some(index));
    }
}

#[test]
fn position_and_index_are_inverse() {
    for row in 0..MATRIX_ROWS {
        for col in 0..MATRIX_COLS {
            let index = cell_index(row, col).unwrap();
            assert_eq!(cell_position(index), Some((row, col)));
        }
    }
}

#[test]
fn layout_is_row_major() {
    // 与原版观测一致：第 0 行是 0..15，第 1 行是 16..31
    assert_eq!(cell_index(0, 0), Some(0));
    assert_eq!(cell_index(0, 15), Some(15));
    assert_eq!(cell_index(1, 0), Some(16));
    assert_eq!(cell_index(15, 0), Some(240));
    assert_eq!(cell_index(15, 15), Some(255));
}

#[test]
fn out_of_range_is_rejected() {
    assert_eq!(cell_position(256), None);
    assert_eq!(cell_position(usize::MAX), None);
    assert_eq!(cell_index(16, 0), None);
    assert_eq!(cell_index(0, 16), None);
    assert_eq!(cell_index(16, 16), None);
}

#[test]
fn cell_of_host_is_identity() {
    // 原版语义：格号 == IP 最后一段
    for host in 0..=255u8 {
        assert_eq!(cell_of_host(host), host as usize);
    }
    assert_eq!(cell_of_ip(Ipv4Addr::new(192, 168, 1, 7)), 7);
    assert_eq!(cell_of_ip(Ipv4Addr::new(10, 0, 0, 0)), 0);
    assert_eq!(cell_of_ip(Ipv4Addr::new(10, 0, 0, 255)), 255);
}

#[test]
fn standard_slots_leave_cell_zero_empty() {
    let prefix = parse_prefix("192.168.1").unwrap();
    let targets = build_targets(&prefix, TargetRange::Standard).unwrap();
    let slots = matrix_slots(&targets);

    assert_eq!(slots.len(), MATRIX_CELLS);
    // `.1`–`.255` 全部落在格 1..=255
    assert!(slots[0].is_none(), "格 0（.0）在 Standard 下必须为空");
    for host in 1..=255u8 {
        let idx = slots[host as usize].unwrap_or_else(|| panic!("格 {host} 应有目标"));
        assert_eq!(targets[idx].octets()[3], host);
    }
    // 205 之外没有其它被占用的格
    assert_eq!(slots.iter().filter(|s| s.is_some()).count(), 255);
}

#[test]
fn full_range_fills_every_cell() {
    let prefix = parse_prefix("10.0.0").unwrap();
    let targets = build_targets(&prefix, TargetRange::Full).unwrap();
    let slots = matrix_slots(&targets);
    assert!(slots.iter().all(|s| s.is_some()), "Full 应填满 256 格");
}

#[test]
fn every_target_maps_to_distinct_cell() {
    let prefix = parse_prefix("172.20.3").unwrap();
    let targets = build_targets(&prefix, TargetRange::Full).unwrap();
    let mut cells: Vec<usize> = targets.iter().map(|ip| cell_of_ip(*ip)).collect();
    let before = cells.len();
    cells.sort_unstable();
    cells.dedup();
    assert_eq!(cells.len(), before, "目标与格号必须一一对应");
}
