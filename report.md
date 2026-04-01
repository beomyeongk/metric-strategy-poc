# Metric Storage Strategy POC — Report

## Overview

A Rust POC that compares three SQLite storage strategies for multi-metric time-series data.  
Each strategy stores **86,400 rows × 20 metrics** into a separate `.db` file (30 days of data at 30s intervals).

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
Each timestamp generates **20 separate rows** → **1,728,000 total rows** in `metric`.

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
| A — JSONB | `a.db` | 29,573,120 | ~28,880 KB | 6.2× |
| B — 1NF | `b.db` | 29,560,832 | ~28,868 KB | 6.2× |
| **C — Binary** | **`c.db`** | **4,796,416** | **~4,684 KB** | **1×** |

> Environment: SQLite (bundled via rusqlite 0.31), WAL mode, no compression, debug build.

---

## Analysis

- **A vs B are nearly identical in size.** Despite B storing data in a normalised form, the overhead of 1,728,000 rows (each with timestamp + name_id + value) matches the size of A's JSON key repetition.
- **C is ~6× smaller than A and B.** Packing values as a raw 40-byte BLOB eliminates all textual overhead: no key names, no commas, no quotes.
- **Trade-off**: C requires the application to own schema parsing logic; A and B are self-describing and can be queried directly with standard SQL/JSON functions.

---

## Transmission Strategies

Each storage strategy was paired with three HTTP transmission formats:

| Method | Format | Description |
|---|---|---|
| **1** | `application/json` — array of objects | `[{"cpu_usage": 123, "mem_free": 456, …}, …]` — fully self-describing; every row carries all key names. |
| **2** | `application/json` — header + value matrix | `{"header": ["cpu_usage", …], "values": [[123, 456, …], …]}` — keys sent once; values as nested arrays. |
| **3** | `multipart/mixed` — schema + binary blob | Part 1 JSON schema `[{"name":"cpu_usage","type":"int","bytes":2}, …]`; Part 2 raw `u16` LE values packed contiguously. |

**Timing scope:**
- **Server** — time to build the response body (DB query + serialization/encoding). Measured just before and after `build_*()`.
- **Client Fetch** — `fetch()` call duration until `arrayBuffer()` resolves (includes network transfer).
- **Client Parse** — time to normalise the received data into `{ key: [values…] }` form, measured after `arrayBuffer()` completes.

---

## HTTP Transmission Benchmark

> 86,400 rows × 20 metrics, localhost, debug build, 10-run average.

| API | Storage | Transmission | Server (ms) | Client Fetch (ms) | Client Parse (ms) | Client Total (ms) |
|---|---|---|---:|---:|---:|---:|
| /a1 | JSONB | Object array | 347 | 381 | 166 | 547 |
| /a2 | JSONB | Header + matrix | 1,459 | 1,481 | 48 | 1,529 |
| /a3 | JSONB | Schema + binary | 1,232 | 1,265 | 49 | 1,314 |
| /b1 | 1NF | Object array | 1,311 | 1,344 | 168 | 1,512 |
| /b2 | 1NF | Header + matrix | 753 | 779 | 49 | 828 |
| /b3 | 1NF | Schema + binary | 542 | 573 | 45 | 618 |
| /c1 | Binary | Object array | 551 | 587 | 175 | 762 |
| /c2 | Binary | Header + matrix | 253 | 280 | 51 | 331 |
| /c3 | Binary | Schema + binary | 23 | 50 | 37 | 87 |

---

## Transmission Analysis

- **Parse cost is format-driven, not storage-driven.** Object-array format (×1) always costs ~165–175 ms client-side regardless of storage strategy, because the parser must iterate every key of every row. Header+matrix (×2) and binary (×3) cut parse time to ~37–51 ms.

- **Server time is storage-driven, not format-driven.** The dominant cost is the DB read shape:
  - **A** reads a single JSONB blob per timestamp (fast DB side, but JSON serialization adds overhead).
  - **B** reads 1,728,000 individual rows (JOIN + value scan is expensive on the server).
  - **C** reads one 40-byte BLOB per timestamp — minimal DB work, no serialization needed.

- **/c3 has an end-to-end latency of 87 ms.** The BLOB is stored pre-encoded as u16 LE, so the server concatenates raw bytes with no conversion; the client uses `DataView.getUint16()` directly on the received buffer with no JSON parsing.

- **/a2 and /a3 are slow on the server (>1,200 ms)** despite JSONB storage, because SQLite's `json()` function re-serializes the internal binary representation back to text for every row — a round-trip that C avoids entirely.

- **The 1NF (B) strategies are consistently mid-tier.** The JOIN overhead (`metric JOIN name_map`) adds meaningful latency even for binary output (/b3: 542 ms server vs /c3: 23 ms).

---

## Metric Names Used

`cpu_usage`, `mem_free`, `disk_read`, `disk_write`, `net_rx`, `net_tx`, `load_avg`,
`swap_used`, `inode_free`, `ctx_switch`, `irq_count`, `softirq`, `forks`,
`procs_run`, `procs_block`, `tcp_conns`, `udp_pkts`, `cache_hit`, `page_fault`, `oom_kill`
