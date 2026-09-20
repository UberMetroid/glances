//! Tests for the programlist plugin — grouping and disagreement collapse.

use crate::plugins::processlist::ProcSample;
use crate::plugins::programlist::{aggregate, register, row_to_value, DISAGREE};

fn sample(pid: u32, name: &str, username: &str, cpu: f64, threads: u64) -> ProcSample {
    ProcSample {
        pid,
        name: name.into(),
        cmdline: format!("{} --run", name),
        username: username.into(),
        num_threads: threads,
        state: 'S',
        nice: 0,
        cpu_percent: cpu,
        memory_percent: 1.0,
        rss: 1000,
        vms: 2000,
        utime: 10,
        stime: 5,
        read_bytes: 100,
        write_bytes: 200,
        cpu_num: 0,
    }
}

#[test]
fn aggregate_groups_and_sums() {
    let samples = vec![
        sample(1, "web", "root", 10.0, 2),
        sample(2, "web", "root", 20.0, 4),
        sample(3, "db", "postgres", 5.0, 1),
    ];
    let rows = aggregate(&samples);
    assert_eq!(rows.len(), 2);
    // Sorted by cpu_percent descending: web (30) before db (5).
    assert_eq!(rows[0].name, "web");
    assert_eq!(rows[0].nprocs, 2);
    assert_eq!(rows[0].num_threads, 6);
    assert_eq!(rows[0].cpu_percent, 30.0);
    assert_eq!(rows[0].childrens, vec![1, 2]);
    assert_eq!(rows[0].username, "root");
    assert_eq!(rows[1].name, "db");
}

#[test]
fn aggregate_collapses_disagreement() {
    let samples = vec![
        sample(1, "web", "root", 1.0, 1),
        sample(2, "web", "www", 1.0, 1),
    ];
    let rows = aggregate(&samples);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].username, DISAGREE);
}

#[test]
fn row_to_value_carries_keys() {
    let rows = aggregate(&[sample(7, "solo", "root", 3.0, 1)]);
    let v = row_to_value(&rows[0]);
    let obj = v.as_object().expect("object");
    for k in [
        "name",
        "cmdline",
        "username",
        "nprocs",
        "num_threads",
        "cpu_percent",
        "memory_percent",
        "memory_info",
        "status",
        "nice",
        "cpu_times",
        "io_counters",
        "childrens",
    ] {
        assert!(obj.contains_key(k), "missing key {}", k);
    }
    assert_eq!(
        obj.get("childrens").and_then(|v| v.as_array()).map(|a| a.len()),
        Some(1)
    );
}

#[test]
fn register_plugin_appears_in_stats() {
    let s = crate::core::stats::GlancesStats::new(1.0);
    register(&s);
    assert!(s.plugin_names().contains(&"programlist"));
}
