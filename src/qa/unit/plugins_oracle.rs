//! Independent oracle for rewritten plugins: expectations derived
//! from ground truth (proc(5) field positions, hand-written fixtures).

use crate::plugins::processcount::{aggregate, parse_proc_stat_counts, parse_stat_line};

#[test]
fn stat_line_fields_by_position() {
    // pid (comm with spaces and (parens)) state ppid pgrp ... num_threads=7.
    let line = "1234 (my proc (x)) R 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 7 0";
    assert_eq!(parse_stat_line(line), Some(('R', 7)));
    let line = "9 (kworker) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 2 0";
    assert_eq!(parse_stat_line(line), Some(('S', 2)));
    assert_eq!(parse_stat_line("garbage"), None);
    assert_eq!(parse_stat_line("1 (x)"), None);
}

#[test]
fn proc_stat_counters_parse() {
    let text = "cpu  1 2 3\nprocs_running 4\nprocs_blocked 5\n";
    assert_eq!(parse_proc_stat_counts(text), (4, 5));
    assert_eq!(parse_proc_stat_counts("nothing here\n"), (0, 0));
}

#[test]
fn live_census_is_self_consistent() {
    let (total, running, sleeping, threads) = aggregate(0, 0);
    assert!(total > 0);
    assert!(running + sleeping <= total);
    assert!(threads >= total);
}
