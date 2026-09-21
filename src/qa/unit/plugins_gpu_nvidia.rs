//! Tests for NVIDIA app attribution — uuid join, apps CSV, the
//! single-card fallback for old drivers.

use crate::plugins::gpu::GpuInfo;
use crate::plugins::gpu_nvidia::{
    apply_apps, parse_apps_csv, parse_nvidia_smi_csv, query_apps,
};
use crate::qa::harness::TempDir;

#[test]
fn parse_nvidia_smi_csv_reads_optional_uuid_eighth_field() {
    let rows = parse_nvidia_smi_csv(
        "00000000:01:00.0, 35, 4524, 16380, 58, 2100, Some GPU, GPU-aaaa\n\
         00000000:06:00.0, 10, 100, 16380, 40, 210, [N/A]\n",
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].gpu_uuid.as_deref(), Some("GPU-aaaa"));
    assert_eq!(rows[1].gpu_uuid, None, "7-field rows stay valid");
}

#[test]
fn parse_apps_csv_keeps_uuid_and_na_mem() {
    let apps = parse_apps_csv(
        "1209, /var/lib/invokeai/.venv/bin/python, 4548, GPU-aaaa\n\
         4321, /usr/bin/ffmpeg, [N/A], GPU-bbbb\n\
         77, /usr/bin/old, 10\n\
         garbage\n",
    );
    assert_eq!(apps.len(), 3);
    assert_eq!(apps[0].pid, 1209);
    assert_eq!(apps[0].mem_mb, Some(4548.0));
    assert_eq!(apps[0].gpu_uuid, "GPU-aaaa");
    assert_eq!(apps[1].mem_mb, None, "[N/A] mem must not fail the row");
    assert_eq!(apps[2].gpu_uuid, "", "3-field rows stay valid");
}

#[test]
fn apply_apps_maps_uuid_and_marks_transcoding() {
    let dir = TempDir::new("gpu-apps");
    let root = dir.path();
    for (pid, comm) in [(4321, "ffmpeg"), (1209, "python")] {
        let p = root.join(pid.to_string());
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("comm"), format!("{comm}\n")).unwrap();
        std::fs::write(p.join("status"), "Name:\tx\nPPid:\t1\n").unwrap();
    }
    let rows = parse_nvidia_smi_csv(
        "00000000:01:00.0, 35, 4524, 16380, 58, 2100, GPU One, GPU-aaaa\n\
         00000000:06:00.0, 10, 100, 16380, 40, 210, GPU Two, GPU-bbbb\n",
    );
    let apps = parse_apps_csv(
        "4321, /usr/bin/ffmpeg, 300, GPU-aaaa\n1209, /venv/bin/python, 4548, GPU-bbbb\n",
    );
    let mut infos = vec![
        GpuInfo { pci: "0000:01:00.0".into(), vendor: "nvidia".into(), ..Default::default() },
        GpuInfo { pci: "0000:06:00.0".into(), vendor: "nvidia".into(), ..Default::default() },
    ];
    apply_apps(&apps, &rows, &mut infos, &root);
    assert_eq!(infos[0].clients.len(), 1);
    assert_eq!(infos[0].clients[0].name, "ffmpeg");
    assert!(infos[0].transcoding);
    assert_eq!(infos[0].transcoding_by.as_deref(), Some("ffmpeg"));
    assert_eq!(infos[1].clients.len(), 1);
    assert_eq!(infos[1].clients[0].name, "python");
    assert!(!infos[1].transcoding, "AI workload is use, not transcoding");
    assert_eq!(infos[1].transcoding_by, None);
}

#[test]
fn apply_apps_without_uuid_needs_exactly_one_nvidia_card() {
    let dir = TempDir::new("gpu-apps-fallback");
    let root = dir.path().to_path_buf();
    let rows = parse_nvidia_smi_csv("00000000:01:00.0, 35, 1, 2, 3, 4, GPU One\n");
    let apps = parse_apps_csv("4321, /usr/bin/ffmpeg, 300\n");
    let mut one = vec![
        GpuInfo { pci: "0000:01:00.0".into(), vendor: "nvidia".into(), ..Default::default() },
    ];
    apply_apps(&apps, &rows, &mut one, &root);
    assert_eq!(one[0].clients.len(), 1, "single card takes unmapped apps");
    let mut two = vec![
        GpuInfo { pci: "0000:01:00.0".into(), vendor: "nvidia".into(), ..Default::default() },
        GpuInfo { pci: "0000:06:00.0".into(), vendor: "nvidia".into(), ..Default::default() },
    ];
    apply_apps(&apps, &rows, &mut two, &root);
    assert!(two[0].clients.is_empty() && two[1].clients.is_empty(),
        "ambiguous target must not guess");
}

#[test]
fn query_apps_never_fails() {
    // Live path: empty vec where nvidia-smi is absent, real apps
    // on NVIDIA hosts. Either way it must not panic.
    for a in query_apps() {
        assert!(!a.name.is_empty());
    }
}
