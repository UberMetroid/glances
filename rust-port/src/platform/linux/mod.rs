//! Linux platform primitives — M0 placeholder.
//!
//! M3 will add: proc_stat, proc_meminfo, proc_net_dev, proc_diskstats,
//! proc_uptime, sys_class_net, sys_class_hwmon readers.

pub fn is_linux() -> bool { cfg!(target_os = "linux") }
