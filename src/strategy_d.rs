use rusqlite::{Connection, params};
use serde_json::{Value, to_string};
use crate::metrics::METRIC_NAMES;

/// Strategy D: header_map + JSONB value array
/// Schema:
///   header_map(id INTEGER PK, headers TEXT)   -- headers: comma-separated metric names
///   metric(timestamp INTEGER, header_id INTEGER, data BLOB)  -- data: SQLite native JSONB array of values
pub fn run(rows: &[[u16; 20]], timestamps: &[i64]) -> rusqlite::Result<()> {
    let conn = Connection::open("d.db")?;

    conn.execute_batch("
        PRAGMA journal_mode=WAL;
        DROP TABLE IF EXISTS metric;
        DROP TABLE IF EXISTS header_map;
        CREATE TABLE header_map (
            id      INTEGER PRIMARY KEY,
            headers TEXT    NOT NULL
        );
        CREATE TABLE metric (
            timestamp INTEGER NOT NULL,
            header_id INTEGER NOT NULL,
            data      BLOB    NOT NULL  -- SQLite native JSONB (binary-encoded JSON array)
        );
    ")?;

    // Insert a single header definition (id: 10)
    // headers: comma-separated metric names
    let headers_str = METRIC_NAMES.join(",");
    conn.execute(
        "INSERT INTO header_map (id, headers) VALUES (?1, ?2)",
        params![10i32, headers_str],
    )?;

    // Insert metric rows: values stored as JSONB array [v0, v1, ..., v19]
    let tx = conn.unchecked_transaction()?;
    {
        let mut ins_metric = tx.prepare(
            "INSERT INTO metric (timestamp, header_id, data) VALUES (?1, ?2, jsonb(?3))"
        )?;
        for (i, row) in rows.iter().enumerate() {
            let arr: Vec<Value> = row.iter().map(|&v| Value::Number(v.into())).collect();
            let json_str = to_string(&arr).unwrap();
            ins_metric.execute(params![timestamps[i], 10i32, json_str])?;
        }
    }
    tx.commit()?;

    Ok(())
}
