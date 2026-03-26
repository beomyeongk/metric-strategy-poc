# Metric Storage Strategy POC — Report

## Overview

A Rust POC that compares three SQLite storage strategies for multi-metric time-series data.  
Each strategy stores **10,000 rows × 20 metrics** into a separate `.db` file.

---

## Strategies

### A — Native JSONB (BLOB)

| Table | Columns |
|---|---|
| `metric` | `timestamp INTEGER`, `data BLOB` |

Each row stores all 20 metrics as a single SQLite native JSONB value (binary-encoded JSON via `jsonb()`).  
Keys are full metric name strings.

### B — First Normal Form (1NF) + `name_map`

| Table | Columns |
|---|---|
| `name_map` | `id INTEGER PK`, `name TEXT` |
| `metric` | `timestamp INTEGER`, `name_id INTEGER`, `value INTEGER` |

Metric names are interned into `name_map` (id 10–29).  
Each timestamp generates **20 separate rows** → **200,000 total rows** in `metric`.

### C — Binary BLOB + `schema_map`

| Table | Columns |
|---|---|
| `schema_map` | `id INTEGER PK`, `schema TEXT` |
| `metric` | `timestamp INTEGER`, `schema_id INTEGER`, `data BLOB` |

The schema is defined once as a JSON array in `schema_map`.  
Each row stores 20 × `u16` as a **40-byte little-endian raw BLOB**.

---

## Results

| Strategy | File | Size (bytes) | Size (KB) | Ratio vs C |
|---|---|---:|---:|---:|
| A — JSONB | `a.db` | 3,432,448 | ~3,352 KB | 6.1× |
| B — 1NF | `b.db` | 3,420,160 | ~3,340 KB | 6.1× |
| **C — Binary** | **`c.db`** | **561,152** | **~548 KB** | **1×** |

> Environment: SQLite (bundled via rusqlite 0.31), WAL mode, no compression, debug build.

---

## Analysis

- **A vs B are nearly identical in size.** Despite B storing data in a normalised form, the overhead of 200,000 rows (each with timestamp + name_id + value) matches the size of A's JSON key repetition.
- **C is ~6× smaller than A and B.** Packing values as a raw 40-byte BLOB eliminates all textual overhead: no key names, no commas, no quotes.
- **Trade-off**: C requires the application to own schema parsing logic; A and B are self-describing and can be queried directly with standard SQL/JSON functions.

---

## Metric Names Used

`cpu_usage`, `mem_free`, `disk_read`, `disk_write`, `net_rx`, `net_tx`, `load_avg`,
`swap_used`, `inode_free`, `ctx_switch`, `irq_count`, `softirq`, `forks`,
`procs_run`, `procs_block`, `tcp_conns`, `udp_pkts`, `cache_hit`, `page_fault`, `oom_kill`
