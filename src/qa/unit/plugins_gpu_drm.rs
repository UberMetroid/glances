//! Tests for GPU open-driver attribution — fdinfo parsing, engine
//! classes, render-node mapping, proc scan, service resolution.

use crate::plugins::gpu_drm::{
    dri_node_to_card, is_video_engine, parse_fdinfo_engines, scan_drm_clients,
};
use crate::plugins::gpu_proc::{is_transcoder_name, resolve_client, service_from_name};
use crate::qa::harness::TempDir;

#[test]
fn parse_fdinfo_engines_keeps_counters_skips_junk() {
    let text = "pos:\t0\nflags:\t02\n\
        drm-driver:\ti915\ndrm-client-id:\t13\n\
        drm-engine-render:\t123456789 ns\ndrm-engine-video:\t987654321 ns\n\
        drm-engine-bogus: not-a-number ns\nnot-an-engine: 1 ns\n";
    let engines = parse_fdinfo_engines(text);
    assert_eq!(engines, vec![
        ("render".to_string(), 123456789),
        ("video".to_string(), 987654321),
    ]);
}

#[test]
fn is_video_engine_splits_video_and_render() {
    for v in ["video", "video-enhance", "dec", "enc", "jpeg", "vpe", "vcn"] {
        assert!(is_video_engine(v), "{v} must be video-class");
    }
    for g in ["render", "gfx", "comp", "copy", "compute"] {
        assert!(!is_video_engine(g), "{g} must not be video-class");
    }
}

#[test]
fn service_from_name_matches_media_servers() {
    assert_eq!(service_from_name("jellyfin"), Some("jellyfin"));
    assert_eq!(service_from_name("jellyfin-ffmpeg"), Some("jellyfin"));
    assert_eq!(service_from_name("EmbyServer"), Some("emby"));
    assert_eq!(service_from_name("Plex Transcoder"), Some("plex"));
    assert_eq!(service_from_name("ffmpeg"), None);
    assert_eq!(service_from_name("python"), None);
}

#[test]
fn is_transcoder_name_matches_transcoders() {
    for t in ["ffmpeg", "jellyfin-ffmpeg", "Plex Transcoder", "HandBrake", "obs", "jellyfin"] {
        assert!(is_transcoder_name(t), "{t} must match");
    }
    for o in ["python", "firefox", "Xorg", "mpv"] {
        assert!(!is_transcoder_name(o), "{o} must not match");
    }
}

#[test]
fn resolve_client_walks_to_media_parent() {
    let dir = TempDir::new("gpu-resolve");
    let root = dir.path();
    // jellyfin(50) -> ffmpeg(100) chain plus an unrelated tree.
    for (pid, comm, ppid) in [(1, "systemd", 0), (50, "jellyfin", 1), (100, "ffmpeg", 50),
                              (200, "python", 1)] {
        let p = root.join(pid.to_string());
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("comm"), format!("{comm}\n")).unwrap();
        std::fs::write(p.join("status"), format!("Name:\t{comm}\nPPid:\t{ppid}\n")).unwrap();
    }
    assert_eq!(
        resolve_client(&root, 100, ""),
        ("ffmpeg".to_string(), Some("jellyfin".to_string())),
        "ffmpeg under jellyfin attributes to jellyfin"
    );
    assert_eq!(
        resolve_client(&root, 200, "/var/lib/invokeai/.venv/bin/python"),
        ("python".to_string(), None),
        "plain process keeps its comm, no service"
    );
    assert_eq!(
        resolve_client(&root, 999, "/usr/lib/plex/Plex Transcoder"),
        ("Plex Transcoder".to_string(), Some("plex".to_string())),
        "missing pid falls back to hint basename"
    );
}

#[test]
fn dri_node_to_card_resolves_render_nodes() {
    use std::os::unix::fs::symlink;
    let dir = TempDir::new("gpu-drm-nodes");
    let root = dir.path();
    for card in ["card1", "card2"] {
        std::fs::create_dir_all(root.join(card).join("device")).unwrap();
    }
    std::fs::create_dir_all(root.join("renderD129")).unwrap();
    // renderD129 shares card1's PCI device; card2 differs.
    std::fs::remove_dir(root.join("card1").join("device")).unwrap();
    std::fs::remove_dir(root.join("card2").join("device")).unwrap();
    symlink("../../../../pci0000:00/0000:00:02.0", root.join("card1").join("device")).unwrap();
    symlink("../../../../pci0000:00/0000:06:00.0", root.join("card2").join("device")).unwrap();
    symlink("../../../../pci0000:00/0000:00:02.0", root.join("renderD129").join("device")).unwrap();
    assert_eq!(dri_node_to_card(&root, "card1").as_deref(), Some("card1"));
    assert_eq!(dri_node_to_card(&root, "renderD129").as_deref(), Some("card1"));
    assert_eq!(dri_node_to_card(&root, "renderD999"), None);
    assert_eq!(dri_node_to_card(&root, "card1-x"), None);
    assert_eq!(dri_node_to_card(&root, "../card1"), None);
}

#[test]
fn scan_drm_clients_merges_fds_per_pid_and_card() {
    use std::os::unix::fs::symlink;
    let dir = TempDir::new("gpu-drm-scan");
    let root = dir.path();
    let proc = root.join("proc");
    std::fs::create_dir_all(proc.join("100/fd")).unwrap();
    std::fs::create_dir_all(proc.join("100/fdinfo")).unwrap();
    symlink("/dev/dri/card1", proc.join("100/fd/7")).unwrap();
    symlink("/dev/dri/renderD129", proc.join("100/fd/9")).unwrap();
    symlink("/dev/null", proc.join("100/fd/3")).unwrap();
    std::fs::write(proc.join("100/fdinfo/7"), "drm-engine-video:\t100 ns\n").unwrap();
    std::fs::write(proc.join("100/fdinfo/9"), "drm-engine-video:\t50 ns\ndrm-engine-render:\t5 ns\n")
        .unwrap();
    let sys = root.join("sys");
    std::fs::create_dir_all(sys.join("card1")).unwrap();
    std::fs::create_dir_all(sys.join("renderD129")).unwrap();
    symlink("pci-A", sys.join("card1").join("device")).unwrap();
    symlink("pci-A", sys.join("renderD129").join("device")).unwrap();
    let found = scan_drm_clients(&proc, &sys);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].pid, 100);
    assert_eq!(found[0].card, "card1");
    assert_eq!(found[0].engines, vec![
        ("render".to_string(), 5),
        ("video".to_string(), 150),
    ]);
}

