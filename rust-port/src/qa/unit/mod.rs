//! Per-module unit tests — one file per source module.

pub mod core_value;
pub mod core_threshold;
pub mod core_timer;
pub mod core_history;
pub mod core_config;
pub mod core_config_dir;
pub mod core_filter;
pub mod core_password;
pub mod core_logger;
pub mod core_hex;
pub mod core_sha256;
pub mod cli_flags;
pub mod cli_parse;
pub mod platform_linux_proc_stat;
pub mod platform_linux_proc_meminfo;
pub mod platform_linux_proc_loadavg;
pub mod platform_linux_proc_uptime;
pub mod platform_linux_proc_net_dev;
pub mod platform_linux_proc_diskstats;
pub mod platform_linux_sys_class_net;
pub mod platform_linux_sys_class_hwmon;
