use rand::Rng;

/// 20 metric names (5-10 chars, with/without underscores)
pub const METRIC_NAMES: [&str; 20] = [
    "cpu_usage",
    "mem_free",
    "disk_read",
    "disk_write",
    "net_rx",
    "net_tx",
    "load_avg",
    "swap_used",
    "inode_free",
    "ctx_switch",
    "irq_count",
    "softirq",
    "forks",
    "procs_run",
    "procs_block",
    "tcp_conns",
    "udp_pkts",
    "cache_hit",
    "page_fault",
    "oom_kill",
];

/// Generate n rows of random metric values (each value: u16 in range 1..=32767)
pub fn generate_values(n: usize) -> Vec<[u16; 20]> {
    let mut rng = rand::thread_rng();
    (0..n)
        .map(|_| {
            let mut row = [0u16; 20];
            for v in row.iter_mut() {
                *v = rng.gen_range(1..=32767);
            }
            row
        })
        .collect()
}
