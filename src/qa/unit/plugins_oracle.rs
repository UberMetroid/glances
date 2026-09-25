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

#[test]
fn device_filter_matrix() {
    use crate::plugins::diskio::{is_partition, should_include};
    assert!(!should_include("sda1"));
    assert!(!should_include("nvme0n1p1"));
    assert!(!should_include("dm-0"));
    assert!(!should_include("md127"));
    assert!(!should_include("zd1234"));
    assert!(should_include("sda"));
    assert!(should_include("sr0"));
    assert!(should_include("nvme0n1"));
    assert!(!is_partition(""));
}

#[test]
fn socket_table_vectors() {
    use crate::plugins::ports::{decode_ipv4, decode_ipv6, parse, split_addr};
    assert_eq!(decode_ipv4("0100007F").as_deref(), Some("127.0.0.1"));
    assert_eq!(decode_ipv4("short").as_deref(), None);
    assert_eq!(
        decode_ipv6("00000000000000000000000001000000").as_deref(),
        Some("0:0:0:0:0:0:0:1")
    );
    assert_eq!(split_addr("0100007F:1F90"), ("127.0.0.1".to_string(), 8080));
    assert_eq!(split_addr("no-colon"), ("no-colon".to_string(), 0));
    let text = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 1234 1 0000000000000000 100 0 0 10 0\n";
    let rows = parse(text, "tcp").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].inode, "1234");
    assert_eq!(rows[0].uid, "0");
}

#[test]
fn connection_tally_vectors() {
    use crate::plugins::connections::tally_rows;
    use crate::plugins::ports::NetRow;
    let tcp = |st: &str| NetRow { st: st.into(), family: "tcp", ..Default::default() };
    let udp = NetRow { st: "07".into(), family: "udp", ..Default::default() };
    let counts = tally_rows(&[tcp("01"), tcp("01"), tcp("0A"), udp]);
    assert_eq!(counts["ESTABLISHED"], 2);
    assert_eq!(counts["LISTEN"], 1);
    assert_eq!(counts["CLOSE"], 0);
}

#[test]
fn irq_parse_vectors() {
    use crate::plugins::irq::parse;
    let text = "           CPU0       CPU1\n  1:         10         20   IO-APIC   1-edge      i8042\nNMI:          0          0   Non-maskable interrupts\n";
    let rows = parse(text);
    assert_eq!(rows, vec![("1_i8042".to_string(), 30), ("NMI".to_string(), 0)]);
}

#[test]
fn interface_role_matrix() {
    use crate::plugins::network::{iface_is_up, iface_role};
    assert!(iface_is_up("up") && iface_is_up("unknown"));
    assert!(!iface_is_up("down") && !iface_is_up("dormant"));
    let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(iface_role("lo", &[]), "loopback");
    assert_eq!(iface_role("eth0", &v(&["127.0.0.2"])), "loopback");
    assert_eq!(iface_role("tailscale0", &[]), "tailscale");
    assert_eq!(iface_role("mytailscale1", &[]), "tailscale");
    assert_eq!(iface_role("eth0", &v(&["100.96.0.2"])), "tailscale");
    assert_eq!(iface_role("eth0", &v(&["192.168.1.5"])), "local");
    assert_eq!(iface_role("eth0", &v(&["10.1.2.3"])), "local");
    assert_eq!(iface_role("eth0", &v(&["172.20.0.9"])), "local");
    assert_eq!(iface_role("eth0", &v(&["8.8.8.8"])), "");
    assert_eq!(iface_role("eth0", &[]), "");
}
