mod metrics;
mod strategy_a;
mod strategy_b;
mod strategy_c;
mod strategy_d;

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

const N_ROWS: usize = 86_400; // 30s interval x 86400 = 30 days

fn main() {
    // Generate N_ROWS timestamps starting from now, 30-second intervals
    let base_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let timestamps: Vec<i64> = (0..N_ROWS as i64).map(|i| base_ts + i * 30).collect();

    // Generate shared data once; reused across all strategies
    println!("Generating data... ({N_ROWS} rows x 20 metrics)");
    let rows = metrics::generate_values(N_ROWS);

    // Strategy A
    print!("[A] Writing JSONB...");
    strategy_a::run(&rows, &timestamps).expect("Strategy A failed");
    println!(" done");

    // Strategy B
    print!("[B] Writing 1NF (name_map)...");
    strategy_b::run(&rows, &timestamps).expect("Strategy B failed");
    println!(" done");

    // Strategy C
    print!("[C] Writing binary (schema_map)...");
    strategy_c::run(&rows, &timestamps).expect("Strategy C failed");
    println!(" done");

    // Strategy D
    print!("[D] Writing JSONB array (header_map)...");
    strategy_d::run(&rows, &timestamps).expect("Strategy D failed");
    println!(" done\n");

    // Print file size comparison
    let size_a = fs::metadata("a.db").map(|m| m.len()).unwrap_or(0);
    let size_b = fs::metadata("b.db").map(|m| m.len()).unwrap_or(0);
    let size_c = fs::metadata("c.db").map(|m| m.len()).unwrap_or(0);
    let size_d = fs::metadata("d.db").map(|m| m.len()).unwrap_or(0);

    println!("========================================");
    println!(" Strategy       File    Size (bytes)");
    println!("----------------------------------------");
    println!(" A (JSONB)      a.db    {:>12}", size_a);
    println!(" B (1NF)        b.db    {:>12}", size_b);
    println!(" C (Binary)     c.db    {:>12}", size_c);
    println!(" D (JSONB arr)  d.db    {:>12}", size_d);
    println!("========================================");

    let min = size_a.min(size_b).min(size_c).min(size_d);
    let label = if min == size_a {
        "A (JSONB)"
    } else if min == size_b {
        "B (1NF)"
    } else if min == size_c {
        "C (Binary)"
    } else {
        "D (JSONB array)"
    };
    println!("\nSmallest strategy: {label} ({min} bytes)");
}
