# Metric Storage Strategy POC — Report

## Overview

A Rust POC that compares four SQLite storage strategies for multi-metric time-series data.  
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

### D — JSONB Array + `header_map`

| Table | Columns |
|---|---|
| `header_map` | `id INTEGER PK`, `headers TEXT` |
| `metric` | `timestamp INTEGER`, `header_id INTEGER`, `data BLOB` |

Header names (metric names, comma-separated) are stored once in `header_map`.  
Each row stores 20 values as a **SQLite native JSONB array** (via `jsonb()`).  
Unlike A, keys are omitted — only the ordered value array is stored per row.  
Unlike C, values are stored as a JSONB number array rather than raw binary.

---

## Results

| Strategy | File | Size (bytes) | Size (KB) | Ratio vs C |
|---|---|---:|---:|---:|
| A — JSONB | `a.db` | 29,573,120 | ~28,880 KB | 6.2× |
| B — 1NF | `b.db` | 29,560,832 | ~28,868 KB | 6.2× |
| **C — Binary** | **`c.db`** | **4,796,416** | **~4,684 KB** | **1×** |
| D — JSONB Array | `d.db` | 11,509,760 | ~11,240 KB | 2.4× |

> Environment: SQLite (bundled via rusqlite 0.31), WAL mode, no compression, debug build.

---

## Storage Analysis

- **A vs B are nearly identical in size.** Despite B storing data in a normalised form, the overhead of 1,728,000 rows (each with timestamp + name_id + value) matches the size of A's JSON key repetition.
- **C is ~6× smaller than A and B.** Packing values as a raw 40-byte BLOB eliminates all textual overhead: no key names, no commas, no quotes.
- **D sits between C and A/B (~2.4× vs C).** By storing only a value array (no key names per row), D avoids the key-repetition overhead of A. However, JSONB encoding of numbers still adds per-value overhead (type tags, integer encoding) compared to C's fixed-width u16 LE binary.
- **Trade-off summary:**
  - A/B: self-describing, SQL/JSON queryable, but large
  - C: smallest footprint, but requires application-level schema parsing
  - D: header names stored once (like C), values as JSONB array — a middle ground between self-description and compactness

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

---

## HTTP Transmission Benchmark

> 86,400 rows × 20 metrics, localhost, debug build, 10-run average.

| API | Storage | Transmission | Server (ms) | Client Fetch (ms) | Client Parse (ms) | Client Total (ms) |
|---|---|---|---:|---:|---:|---:|
| /a1 | JSONB | Object array | 347 | 382 | 172 | 554 |
| /a2 | JSONB | Header + matrix | 1,438 | 1,466 | 49 | 1,515 |
| /a3 | JSONB | Schema + binary | 1,227 | 1,255 | 37 | 1,292 |
| /b1 | 1NF | Object array | 1,307 | 1,343 | 168 | 1,511 |
| /b2 | 1NF | Header + matrix | 747 | 775 | 50 | 825 |
| /b3 | 1NF | Schema + binary | 540 | 566 | 38 | 604 |
| /c1 | Binary | Object array | 548 | 588 | 167 | 755 |
| /c2 | Binary | Header + matrix | 243 | 279 | 49 | 328 |
| /c3 | Binary | Schema + binary | 16 | 50 | 37 | 87 |
| /d1 | JSONB Array | Object array | 721 | 768 | 167 | 935 |
| /d2 | JSONB Array | Header + matrix | 112 | 149 | 49 | 198 |
| /d3 | JSONB Array | Schema + binary | 206 | 239 | 37 | 276 |

---

## Transmission Analysis

- **Parse cost is format-driven, not storage-driven.** Object-array format (×1) always costs ~165–175 ms client-side regardless of storage strategy, because the JS engine must expensive iteration for every key of every row. Header+matrix (×2) and binary (×3) formats consistently cut parse time down to ~37–50 ms.

- **Server time is storage-driven, but D2 shows a unique advantage.** 
  - **C3 remains the absolute leader (16ms server / 87ms total)** because it bypasses all serialization; it's a pure raw BLOB concatenation.
  - **D2 (JSONB Array + Matrix) is a very strong runner-up (112ms server / 198ms total).** It is even faster on the server than C2 (243ms). This is because Strategy D stores data as a JSONB array; `SELECT json(data)` in SQLite is extremely fast at converting its internal binary array to a JSON text array, avoiding the manual U16-to-JSON serialization loop that C2 has to perform in Rust.
  - **D1 is slower than A1 (721ms vs 347ms).** Even though D's storage is smaller, building an object array requires the server to "inflate" the value-only array back into key-value pairs using the `header_map`, which adds significant CPU overhead compared to A1 where those pairs are already stored.

- **/a2 and /a3 are the slowest (>1,200ms).** Despite using JSONB, re-serializing an object-key-heavy binary format back into text for every row causes massive server-side latency.

- **Strategy C (Binary) combined with Format 3 (Multipart Binary) provides the best overall performance**, but **Strategy D with Format 2** offers an excellent balance: it's nearly as fast, much smaller than A/B, and much easier to query via SQL than C.

---

## Metric Names Used

`cpu_usage`, `mem_free`, `disk_read`, `disk_write`, `net_rx`, `net_tx`, `load_avg`,
`swap_used`, `inode_free`, `ctx_switch`, `irq_count`, `softirq`, `forks`,
`procs_run`, `procs_block`, `tcp_conns`, `udp_pkts`, `cache_hit`, `page_fault`, `oom_kill`
