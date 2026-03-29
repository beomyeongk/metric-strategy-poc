use rusqlite::Connection;
use serde_json::{Map, Value, json};
use std::io::Write;
use std::time::Instant;
use tiny_http::{Header, Method, Response, Server};

// ── HTML page (embedded from static/index.html at compile time) ─────────────

const HTML: &str = include_str!("../static/index.html");
const DB_PATH: &str = "a.db";
const DB_B_PATH: &str = "b.db";
const DB_C_PATH: &str = "c.db";

// ── /api/a1 ────────────────────────────────────────────────────────────────
// Returns: [{"key1": val, "key2": val, ...}, {...}, ...]
fn build_a1() -> Vec<u8> {
    let conn = Connection::open(DB_PATH).expect("Cannot open a.db");
    let mut stmt = conn
        .prepare("SELECT json(data) FROM metric ORDER BY rowid")
        .expect("prepare failed");
    // Forward raw JSON text directly – no re-parsing needed
    let rows: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .expect("query failed")
        .filter_map(|r| r.ok())
        .collect();
    format!("[{}]", rows.join(",")).into_bytes()
}

// ── /api/a2 ────────────────────────────────────────────────────────────────
// Returns: {"header": ["key1", ...], "values": [[v1, v2, ...], [...]]}
fn build_a2() -> Vec<u8> {
    let conn = Connection::open(DB_PATH).expect("Cannot open a.db");
    let mut stmt = conn
        .prepare("SELECT json(data) FROM metric ORDER BY rowid")
        .expect("prepare failed");
    let mut rows = stmt.query([]).expect("query failed");

    let mut keys: Vec<String> = Vec::new();
    let mut value_rows: Vec<String> = Vec::new();

    while let Some(row) = rows.next().expect("row error") {
        let json_str: String = row.get(0).expect("col error");
        let obj: Map<String, Value> = serde_json::from_str(&json_str).expect("parse error");
        if keys.is_empty() {
            keys = obj.keys().cloned().collect();
        }
        let vals: Vec<&Value> = keys.iter().map(|k| &obj[k]).collect();
        value_rows.push(serde_json::to_string(&vals).unwrap());
    }

    let header_json = serde_json::to_string(&keys).unwrap();
    format!(
        "{{\"header\":{},\"values\":[{}]}}",
        header_json,
        value_rows.join(",")
    )
    .into_bytes()
}

// ── /api/a3 ────────────────────────────────────────────────────────────────
// Returns: multipart/mixed
//   Part 1 application/json: schema array [{"bytes":2,"name":"key","type":"int"}, ...]
//   Part 2 application/octet-stream: all values as packed u16 little-endian
fn build_a3(boundary: &str) -> Vec<u8> {
    let conn = Connection::open(DB_PATH).expect("Cannot open a.db");
    let mut stmt = conn
        .prepare("SELECT json(data) FROM metric ORDER BY rowid")
        .expect("prepare failed");
    let mut rows = stmt.query([]).expect("query failed");

    let mut keys: Vec<String> = Vec::new();
    let mut binary: Vec<u8> = Vec::with_capacity(86_400 * 20 * 2);

    while let Some(row) = rows.next().expect("row error") {
        let json_str: String = row.get(0).expect("col error");
        let obj: Map<String, Value> = serde_json::from_str(&json_str).expect("parse error");
        if keys.is_empty() {
            keys = obj.keys().cloned().collect();
        }
        for key in &keys {
            let val = obj[key].as_u64().unwrap_or(0) as u16;
            binary.extend_from_slice(&val.to_le_bytes());
        }
    }

    // Build schema JSON from first-row keys
    let schema: Vec<Value> = keys
        .iter()
        .map(|name| json!({"bytes": 2, "name": name, "type": "int"}))
        .collect();
    let schema_json = serde_json::to_string(&schema).unwrap();

    // Assemble multipart body
    let mut body: Vec<u8> = Vec::new();
    // Part 1: JSON schema
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Type: application/json\r\n\r\n");
    body.extend_from_slice(schema_json.as_bytes());
    body.extend_from_slice(b"\r\n");
    // Part 2: binary blob
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(&binary);
    body.extend_from_slice(b"\r\n");
    // Final boundary
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

// ── /api/b1 ────────────────────────────────────────────────────────────────
// Returns: [{"key1": val, "key2": val, ...}, {...}, ...]
// Requires JOIN between metric and name_map.
fn build_b1() -> Vec<u8> {
    let conn = Connection::open(DB_B_PATH).expect("Cannot open b.db");
    // Fetch all (timestamp, name, value) ordered so we get 20 rows per timestamp group
    let mut stmt = conn
        .prepare(
            "SELECT m.timestamp, n.name, m.value \
             FROM metric m \
             JOIN name_map n ON m.name_id = n.id \
             ORDER BY m.timestamp, m.name_id",
        )
        .expect("prepare failed");

    let mut rows = stmt.query([]).expect("query failed");

    let mut objects: Vec<String> = Vec::new();
    let mut current_ts: Option<i64> = None;
    let mut pairs: Vec<String> = Vec::new(); // "\"name\":val" fragments

    while let Some(row) = rows.next().expect("row error") {
        let ts: i64 = row.get(0).expect("col ts");
        let name: String = row.get(1).expect("col name");
        let val: i64 = row.get(2).expect("col val");

        if let Some(prev_ts) = current_ts {
            if ts != prev_ts {
                // Flush previous group
                objects.push(format!("{{{}}}", pairs.join(",")));
                pairs.clear();
            }
        }
        current_ts = Some(ts);
        pairs.push(format!("\"{name}\":{val}"));
    }
    // Flush last group
    if !pairs.is_empty() {
        objects.push(format!("{{{}}}", pairs.join(",")));
    }

    format!("[{}]", objects.join(",")).into_bytes()
}

// ── /api/b2 ────────────────────────────────────────────────────────────────
// Returns: {"header": ["key1", ...], "values": [[v1, v2, ...], [...]]}
// Keys are fixed (20 metric names), fetched from name_map ORDER BY id.
fn build_b2() -> Vec<u8> {
    let conn = Connection::open(DB_B_PATH).expect("Cannot open b.db");

    // Header: names in id order
    let mut key_stmt = conn
        .prepare("SELECT name FROM name_map ORDER BY id")
        .expect("prepare failed");
    let keys: Vec<String> = key_stmt
        .query_map([], |r| r.get(0))
        .expect("query failed")
        .filter_map(|r| r.ok())
        .collect();
    let n_keys = keys.len();

    // Values: all metric values ordered by (timestamp, name_id)
    let mut val_stmt = conn
        .prepare("SELECT value FROM metric ORDER BY timestamp, name_id")
        .expect("prepare failed");
    let all_values: Vec<i64> = val_stmt
        .query_map([], |r| r.get(0))
        .expect("query failed")
        .filter_map(|r| r.ok())
        .collect();

    let header_json = serde_json::to_string(&keys).unwrap();
    let value_rows: Vec<String> = all_values
        .chunks(n_keys)
        .map(|chunk| {
            let vals: Vec<i64> = chunk.to_vec();
            serde_json::to_string(&vals).unwrap()
        })
        .collect();

    format!(
        "{{\"header\":{},\"values\":[{}]}}",
        header_json,
        value_rows.join(",")
    )
    .into_bytes()
}

// ── /api/b3 ────────────────────────────────────────────────────────────────
// Returns: multipart/mixed
//   Part 1 application/json: schema [{"bytes":2,"name":"k","type":"int"}, ...]
//   Part 2 application/octet-stream: all values packed as u16 LE
fn build_b3(boundary: &str) -> Vec<u8> {
    let conn = Connection::open(DB_B_PATH).expect("Cannot open b.db");

    // Schema from name_map ORDER BY id
    let mut key_stmt = conn
        .prepare("SELECT name FROM name_map ORDER BY id")
        .expect("prepare failed");
    let keys: Vec<String> = key_stmt
        .query_map([], |r| r.get(0))
        .expect("query failed")
        .filter_map(|r| r.ok())
        .collect();

    // Binary values
    let mut val_stmt = conn
        .prepare("SELECT value FROM metric ORDER BY timestamp, name_id")
        .expect("prepare failed");
    let mut binary: Vec<u8> = Vec::with_capacity(86_400 * 20 * 2);
    val_stmt
        .query_map([], |r| r.get::<_, i64>(0))
        .expect("query failed")
        .filter_map(|r| r.ok())
        .for_each(|v| binary.extend_from_slice(&(v as u16).to_le_bytes()));

    // Build schema JSON
    let schema: Vec<Value> = keys
        .iter()
        .map(|name| json!({"bytes": 2, "name": name, "type": "int"}))
        .collect();
    let schema_json = serde_json::to_string(&schema).unwrap();

    // Assemble multipart body
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Type: application/json\r\n\r\n");
    body.extend_from_slice(schema_json.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(&binary);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

// ── helpers shared by c1/c2/c3 ───────────────────────────────────────────────
// Load key names from schema_map (one row, id=10) into a Vec<String>.
fn load_c_keys(conn: &Connection) -> Vec<String> {
    let schema_str: String = conn
        .query_row("SELECT schema FROM schema_map WHERE id = 10", [], |r| {
            r.get(0)
        })
        .expect("schema_map query failed");
    let arr: Vec<serde_json::Map<String, Value>> =
        serde_json::from_str(&schema_str).expect("schema JSON parse failed");
    arr.into_iter()
        .map(|obj| obj["name"].as_str().unwrap().to_string())
        .collect()
}

// ── /api/c1 ────────────────────────────────────────────────────────────────
// Returns: [{"key1": val, "key2": val, ...}, {...}, ...]
fn build_c1() -> Vec<u8> {
    let conn = Connection::open(DB_C_PATH).expect("Cannot open c.db");
    let keys = load_c_keys(&conn);
    let n = keys.len();

    let mut stmt = conn
        .prepare("SELECT data FROM metric ORDER BY rowid")
        .expect("prepare failed");
    let objects: Vec<String> = stmt
        .query_map([], |r| r.get::<_, Vec<u8>>(0))
        .expect("query failed")
        .filter_map(|r| r.ok())
        .map(|blob| {
            let pairs: Vec<String> = (0..n)
                .map(|i| {
                    let v = u16::from_le_bytes([blob[i * 2], blob[i * 2 + 1]]);
                    format!("\"{}\":{}", keys[i], v)
                })
                .collect();
            format!("{{{}}}", pairs.join(","))
        })
        .collect();

    format!("[{}]", objects.join(",")).into_bytes()
}

// ── /api/c2 ────────────────────────────────────────────────────────────────
// Returns: {"header": ["key1", ...], "values": [[v1, v2, ...], [...]]}
fn build_c2() -> Vec<u8> {
    let conn = Connection::open(DB_C_PATH).expect("Cannot open c.db");
    let keys = load_c_keys(&conn);
    let n = keys.len();

    let mut stmt = conn
        .prepare("SELECT data FROM metric ORDER BY rowid")
        .expect("prepare failed");
    let value_rows: Vec<String> = stmt
        .query_map([], |r| r.get::<_, Vec<u8>>(0))
        .expect("query failed")
        .filter_map(|r| r.ok())
        .map(|blob| {
            let nums: Vec<u16> = (0..n)
                .map(|i| u16::from_le_bytes([blob[i * 2], blob[i * 2 + 1]]))
                .collect();
            serde_json::to_string(&nums).unwrap()
        })
        .collect();

    let header_json = serde_json::to_string(&keys).unwrap();
    format!(
        "{{\"header\":{},\"values\":[{}]}}",
        header_json,
        value_rows.join(",")
    )
    .into_bytes()
}

// ── /api/c3 ────────────────────────────────────────────────────────────────
// Returns: multipart/mixed
//   Part 1 application/json: schema from schema_map verbatim
//   Part 2 application/octet-stream: all BLOBs concatenated (already u16 LE)
fn build_c3(boundary: &str) -> Vec<u8> {
    let conn = Connection::open(DB_C_PATH).expect("Cannot open c.db");

    // Schema: reuse the stored JSON string directly (keys in insertion order)
    let schema_json: String = conn
        .query_row("SELECT schema FROM schema_map WHERE id = 10", [], |r| {
            r.get(0)
        })
        .expect("schema_map query failed");

    // Binary: concatenate all BLOBs in rowid order
    let mut stmt = conn
        .prepare("SELECT data FROM metric ORDER BY rowid")
        .expect("prepare failed");
    let mut binary: Vec<u8> = Vec::with_capacity(86_400 * 20 * 2);
    stmt.query_map([], |r| r.get::<_, Vec<u8>>(0))
        .expect("query failed")
        .filter_map(|r| r.ok())
        .for_each(|blob| binary.extend_from_slice(&blob));

    // Assemble multipart body
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Type: application/json\r\n\r\n");
    body.extend_from_slice(schema_json.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(&binary);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

// ── Server ─────────────────────────────────────────────────────────────────

pub fn run() {
    let server = Server::http("0.0.0.0:8080").expect("Failed to bind server");
    println!("Listening on http://0.0.0.0:8080");

    for request in server.incoming_requests() {
        let url = request.url().to_string();
        let method = request.method().clone();

        match (method, url.as_str()) {
            // ── HTML page ───────────────────────────────────────────────
            (Method::Get, "/") => {
                let response = Response::from_string(HTML).with_header(
                    Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap(),
                );
                let _ = request.respond(response);
            }

            // ── /api/a1 ─────────────────────────────────────────────────
            (Method::Get, "/api/a1") => {
                let t = Instant::now();
                let body = build_a1();
                let response = Response::from_data(body)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                print!("a1({}) ", t.elapsed().as_millis());
                let _ = std::io::stdout().flush();
            }

            // ── /api/a2 ─────────────────────────────────────────────────
            (Method::Get, "/api/a2") => {
                let t = Instant::now();
                let body = build_a2();
                let response = Response::from_data(body)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                print!("a2({}) ", t.elapsed().as_millis());
                let _ = std::io::stdout().flush();
            }

            // ── /api/a3 (multipart) ──────────────────────────────────────
            (Method::Get, "/api/a3") => {
                let t = Instant::now();
                let boundary = "MetricBoundary42";
                let body = build_a3(boundary);
                let content_type = format!("multipart/mixed; boundary={boundary}");
                let response = Response::from_data(body).with_header(
                    Header::from_bytes("Content-Type", content_type.as_str()).unwrap(),
                );
                let _ = request.respond(response);
                print!("a3({}) ", t.elapsed().as_millis());
                let _ = std::io::stdout().flush();
            }

            // ── /api/b1 ─────────────────────────────────────────────────
            (Method::Get, "/api/b1") => {
                let t = Instant::now();
                let body = build_b1();
                let response = Response::from_data(body)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                print!("b1({}) ", t.elapsed().as_millis());
                let _ = std::io::stdout().flush();
            }

            // ── /api/b2 ─────────────────────────────────────────────────
            (Method::Get, "/api/b2") => {
                let t = Instant::now();
                let body = build_b2();
                let response = Response::from_data(body)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                print!("b2({}) ", t.elapsed().as_millis());
                let _ = std::io::stdout().flush();
            }

            // ── /api/b3 (multipart) ──────────────────────────────────────
            (Method::Get, "/api/b3") => {
                let t = Instant::now();
                let boundary = "MetricBoundary42";
                let body = build_b3(boundary);
                let content_type = format!("multipart/mixed; boundary={boundary}");
                let response = Response::from_data(body).with_header(
                    Header::from_bytes("Content-Type", content_type.as_str()).unwrap(),
                );
                let _ = request.respond(response);
                print!("b3({}) ", t.elapsed().as_millis());
                let _ = std::io::stdout().flush();
            }

            // ── /api/c1 ─────────────────────────────────────────────────
            (Method::Get, "/api/c1") => {
                let t = Instant::now();
                let body = build_c1();
                let response = Response::from_data(body)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                print!("c1({}) ", t.elapsed().as_millis());
                let _ = std::io::stdout().flush();
            }

            // ── /api/c2 ─────────────────────────────────────────────────
            (Method::Get, "/api/c2") => {
                let t = Instant::now();
                let body = build_c2();
                let response = Response::from_data(body)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                print!("c2({}) ", t.elapsed().as_millis());
                let _ = std::io::stdout().flush();
            }

            // ── /api/c3 (multipart) ──────────────────────────────────────
            (Method::Get, "/api/c3") => {
                let t = Instant::now();
                let boundary = "MetricBoundary42";
                let body = build_c3(boundary);
                let content_type = format!("multipart/mixed; boundary={boundary}");
                let response = Response::from_data(body).with_header(
                    Header::from_bytes("Content-Type", content_type.as_str()).unwrap(),
                );
                let _ = request.respond(response);
                print!("c3({}) ", t.elapsed().as_millis());
                let _ = std::io::stdout().flush();
            }

            // ── 404 ────────────────────────────────────────────────────
            _ => {
                let response = Response::from_string("Not Found").with_status_code(404);
                let _ = request.respond(response);
            }
        }
    }
}
