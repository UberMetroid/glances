//! Tests for the GPU plugin — vendor detection + Value shape.

use crate::core::plugin::Plugin;
use crate::core::stats::GlancesStats;
use crate::core::value::Value;
use crate::plugins::gpu::{classify_kind, vendor_from_driver, GpuInfo, NAME};
use crate::plugins::gpu_drm::GpuClient;
use crate::plugins::gpu_format::{assign_gpu_ids, gpu_to_value, mem_pct, sort_gpus};
use crate::plugins::gpu_nvidia::{apply, normalize_pci, parse_nvidia_smi_csv, query_nvidia_smi};

#[test]
fn name_and_register() {
    let s = GlancesStats::new(1.0);
    crate::plugins::gpu::register(&s);
    assert!(s.plugin_names().contains(&NAME));
}

#[test]
fn get_key_returns_gpu_id() {
    let p = crate::plugins::gpu::GpuPlugin::new();
    assert_eq!(p.get_key(), Some("gpu_id"));
}

#[test]
fn vendor_from_driver_maps_known_drivers() {
    assert_eq!(vendor_from_driver("amdgpu"), "amd");
    assert_eq!(vendor_from_driver("i915"), "intel");
    assert_eq!(vendor_from_driver("nouveau"), "nvidia");
    assert_eq!(vendor_from_driver("tegra-drm"), "tegra");
    assert_eq!(vendor_from_driver("radeon"), "amd");
    // Unknown drivers are preserved verbatim.
    assert_eq!(vendor_from_driver("vmwgfx"), "vmware");
    assert_eq!(vendor_from_driver("my_custom_driver"), "my_custom_driver");
}

#[test]
fn gpu_to_value_emits_canonical_keys() {
    let g = GpuInfo {
        gpu_id: "amd0".into(),
        pci: "0000:03:00.0".into(),
        vendor: "amd".into(),
        name: "Radeon RX 7900 XT".into(),
        kind: "external".into(),
        util_pct: Some(42.0),
        freq_mhz: Some(2400.0),
        mem_used_mb: Some(8192.0),
        mem_total_mb: Some(16384.0),
        temp_c: Some(65.0),
        card: "card1".into(),
        clients: vec![GpuClient { pid: 4321, name: "ffmpeg".into(), service: Some("jellyfin".into()),
            mem_mb: None, transcoding: true }],
        transcoding: true,
        transcoding_by: Some("jellyfin".into()),
    };
    let v = gpu_to_value(&g);
    let obj = v.as_object().expect("object");
    assert_eq!(obj.get("gpu_id").and_then(Value::as_str), Some("amd0"));
    assert_eq!(obj.get("pci").and_then(Value::as_str), Some("0000:03:00.0"));
    assert_eq!(obj.get("key").and_then(Value::as_str), Some("gpu_id"));
    assert_eq!(obj.get("proc").and_then(Value::as_f64), Some(42.0));
    assert_eq!(obj.get("mem").and_then(Value::as_f64), Some(50.0));
    assert_eq!(obj.get("temperature").and_then(Value::as_f64), Some(65.0));
    assert_eq!(obj.get("vendor").and_then(Value::as_str), Some("amd"));
    assert_eq!(obj.get("name").and_then(Value::as_str), Some("Radeon RX 7900 XT"));
    assert_eq!(obj.get("util_pct").and_then(Value::as_f64), Some(42.0));
    assert_eq!(obj.get("freq_mhz").and_then(Value::as_f64), Some(2400.0));
    assert_eq!(obj.get("kind").and_then(Value::as_str), Some("external"));
    assert_eq!(obj.get("mem_used_mb").and_then(Value::as_f64), Some(8192.0));
    assert_eq!(obj.get("mem_total_mb").and_then(Value::as_f64), Some(16384.0));
    assert_eq!(obj.get("temp_c").and_then(Value::as_f64), Some(65.0));
    assert!(matches!(obj.get("transcoding"), Some(Value::Bool(true))));
    assert_eq!(obj.get("transcoding_by").and_then(Value::as_str), Some("jellyfin"));
    assert!(obj.get("card").is_none(), "card stays internal");
    let clients = obj.get("clients").and_then(Value::as_array).expect("clients array");
    assert_eq!(clients.len(), 1);
    let c = clients[0].as_object().expect("client object");
    assert!(matches!(c.get("pid"), Some(Value::Uint(4321))));
    assert_eq!(c.get("name").and_then(Value::as_str), Some("ffmpeg"));
    assert_eq!(c.get("service").and_then(Value::as_str), Some("jellyfin"));
    assert!(matches!(c.get("mem_mb"), Some(Value::Null)));
    assert!(matches!(c.get("transcoding"), Some(Value::Bool(true))));
}

#[test]
fn gpu_to_value_handles_missing_optional_fields() {
    let g = GpuInfo {
        gpu_id: "intel0".into(),
        pci: "0000:00:02.0".into(),
        vendor: "intel".into(),
        name: "Meteor Lake".into(),
        kind: "internal".into(),
        util_pct: None,
        freq_mhz: None,
        mem_used_mb: None,
        mem_total_mb: None,
        temp_c: None,
        card: "card0".into(),
        clients: Vec::new(),
        transcoding: false,
        transcoding_by: None,
    };
    let v = gpu_to_value(&g);
    let obj = v.as_object().unwrap();
    // Missing fields must serialize as JSON null (Value::Null), not 0.
    assert!(matches!(obj.get("util_pct"), Some(Value::Null)));
    assert!(matches!(obj.get("freq_mhz"), Some(Value::Null)));
    assert!(matches!(obj.get("proc"), Some(Value::Null)));
    assert!(matches!(obj.get("mem"), Some(Value::Null)));
    assert!(matches!(obj.get("temperature"), Some(Value::Null)));
    assert!(matches!(obj.get("transcoding"), Some(Value::Bool(false))));
    assert!(matches!(obj.get("transcoding_by"), Some(Value::Null)));
    assert!(obj.get("clients").and_then(Value::as_array).unwrap().is_empty());
}

#[test]
fn plugin_update_emits_array_even_without_gpus() {
    let mut p = crate::plugins::gpu::GpuPlugin::new();
    p.update().expect("update must not error");
    assert!(p.stats().as_array().is_some(), "stats always an array");
}

#[test]
fn plugin_reset_clears_stats_to_empty_array() {
    let mut p = crate::plugins::gpu::GpuPlugin::new();
    p.update().expect("update ok");
    p.reset();
    assert!(p.stats().as_array().unwrap().is_empty());
}

#[test]
fn classify_kind_splits_internal_and_external() {
    // Firmware label wins for any vendor.
    assert_eq!(classify_kind("nvidia", "0000:06:00.0", Some("Onboard - Video")), "internal");
    // Intel iGPU lives at PCI 00:02.x, always.
    assert_eq!(classify_kind("intel", "0000:00:02.0", None), "internal");
    assert_eq!(classify_kind("intel", "00:02.0", None), "internal");
    // Intel Arc dGPUs live elsewhere -> external.
    assert_eq!(classify_kind("intel", "0000:03:00.0", None), "external");
    // Tegra is SoC-integrated.
    assert_eq!(classify_kind("tegra", "ahb:gpu", None), "internal");
    // Discrete and unknown default to external.
    assert_eq!(classify_kind("nvidia", "0000:01:00.0", None), "external");
    assert_eq!(classify_kind("amd", "0000:03:00.0", None), "external");
    assert_eq!(classify_kind("unknown", "card9", None), "external");
}

#[test]
fn sort_gpus_internal_first_then_by_name() {
    let mk = |name: &str, kind: &str| GpuInfo {
        gpu_id: "x".into(), pci: "x".into(), vendor: "v".into(), name: name.into(),
        kind: kind.into(), ..Default::default()
    };
    let mut g = vec![mk("card2", "external"), mk("card0", "external"), mk("Onboard", "internal")];
    sort_gpus(&mut g);
    let names: Vec<&str> = g.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, vec!["Onboard", "card0", "card2"]);
}

#[test]
fn normalize_pci_unifies_smi_and_sysfs_domains() {
    assert_eq!(normalize_pci("00000000:01:00.0"), "0000:01:00.0");
    assert_eq!(normalize_pci("0000:06:00.0"), "0000:06:00.0");
    assert_eq!(normalize_pci("  00000000:0a:00.0  "), "0000:0a:00.0");
    assert_eq!(normalize_pci("not-a-pci-id"), "not-a-pci-id");
}

#[test]
fn parse_nvidia_smi_csv_skips_bad_lines_keeps_na_rows() {
    let text = "00000000:01:00.0, 35, 4524, 16380, 58, 2100, NVIDIA GeForce RTX 4060 Ti\n\
                garbage-line\n\
                00000000:06:00.0, [N/A], 100, 16380, [N/A], 210, [N/A]\n";
    let rows = parse_nvidia_smi_csv(text);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].pci, "0000:01:00.0");
    assert_eq!(rows[0].util_pct, Some(35.0));
    assert_eq!(rows[0].temp_c, Some(58.0));
    assert_eq!(rows[0].name.as_deref(), Some("NVIDIA GeForce RTX 4060 Ti"));
    assert_eq!(rows[1].name, None, "[N/A] name must not clobber");
    // [N/A] counters become None but the row survives.
    assert_eq!(rows[1].util_pct, None);
    assert_eq!(rows[1].mem_used_mb, Some(100.0));
}

#[test]
fn apply_joins_by_pci_and_never_clobbers_with_na() {
    let mut infos = vec![GpuInfo {
        gpu_id: "nvidia0".into(), pci: "0000:01:00.0".into(),
        vendor: "nvidia".into(), name: "card2".into(),
        kind: "external".into(), freq_mhz: Some(2200.0),
        ..Default::default()
    }];
    let rows = parse_nvidia_smi_csv("00000000:01:00.0, 35, 4524, 16380, [N/A], 2100, NVIDIA GeForce RTX 4060 Ti\n");
    apply(&rows, &mut infos);
    let g = &infos[0];
    assert_eq!(g.util_pct, Some(35.0));
    assert_eq!(g.mem_used_mb, Some(4524.0));
    assert_eq!(g.name, "NVIDIA GeForce RTX 4060 Ti", "smi name replaces cardN");
    assert_eq!(g.temp_c, None, "[N/A] temp must not clobber");
    // Unmatched cards are untouched.
    let mut other = vec![GpuInfo {
        gpu_id: "intel0".into(), pci: "0000:00:02.0".into(),
        vendor: "intel".into(), name: "iGPU".into(),
        kind: "internal".into(), ..Default::default()
    }];
    apply(&rows, &mut other);
    assert_eq!(other[0].util_pct, None);
}

#[test]
fn query_nvidia_smi_never_fails_and_rows_are_sane() {
    // Live path: empty vec where nvidia-smi is absent, real rows on
    // NVIDIA hosts. Either way it must not panic or error.
    for r in query_nvidia_smi() {
        assert!(r.util_pct.is_none_or(|v| (0.0..=100.0).contains(&v)), "util range: {:?}", r);
        assert!(r.temp_c.is_none_or(|v| (-50.0..=120.0).contains(&v)), "temp range: {:?}", r);
    }
}

#[test]
fn assign_gpu_ids_numbers_per_vendor_in_pci_order() {
    let mk = |vendor: &str, pci: &str| GpuInfo {
        gpu_id: String::new(), pci: pci.into(), vendor: vendor.into(),
        name: "n".into(), kind: "external".into(), ..Default::default()
    };
    // Card order need not be PCI order (card0 = 06:00 here); numbering
    // follows PCI so it matches nvidia-smi index order. Ids stay
    // attached to their card (asserted in input order).
    let mut infos = vec![
        mk("nvidia", "0000:06:00.0"),
        mk("intel", "0000:00:02.0"),
        mk("nvidia", "0000:01:00.0"),
        mk("weird vendor!", "x"),
    ];
    assign_gpu_ids(&mut infos);
    let ids: Vec<&str> = infos.iter().map(|g| g.gpu_id.as_str()).collect();
    assert_eq!(ids, vec!["nvidia1", "intel0", "nvidia0", "gpu0"]);
}

#[test]
fn mem_pct_needs_both_counters_and_positive_total() {
    assert_eq!(mem_pct(Some(8192.0), Some(16384.0)), Some(50.0));
    assert_eq!(mem_pct(None, Some(16384.0)), None);
    assert_eq!(mem_pct(Some(100.0), None), None);
    assert_eq!(mem_pct(Some(100.0), Some(0.0)), None);
}
