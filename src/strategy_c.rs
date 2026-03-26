use rusqlite::{Connection, params};
use serde_json::{json, to_string};
use crate::metrics::METRIC_NAMES;

/// Strategy C: schema_map + raw binary BLOB
/// Schema:
///   schema_map(id INTEGER PK, schema TEXT)
///   metric(timestamp INTEGER, schema_id INTEGER, data BLOB)
pub fn run(rows: &[[u16; 20]], timestamps: &[i64]) -> rusqlite::Result<()> {
    let conn = Connection::open("c.db")?;

    conn.execute_batch("
        PRAGMA journal_mode=WAL;
        DROP TABLE IF EXISTS metric;
        DROP TABLE IF EXISTS schema_map;
        CREATE TABLE schema_map (
            id     INTEGER PRIMARY KEY,
            schema TEXT    NOT NULL
        );
        CREATE TABLE metric (
            timestamp INTEGER NOT NULL,
            schema_id INTEGER NOT NULL,
            data      BLOB    NOT NULL
        );
    ")?;

    // Insert a single schema definition (id: 10)
    let schema_json: Vec<_> = METRIC_NAMES
        .iter()
        .map(|&name| json!({"name": name, "type": "int", "bytes": 2}))
        .collect();
    let schema_str = to_string(&schema_json).unwrap();
    conn.execute(
        "INSERT INTO schema_map (id, schema) VALUES (?1, ?2)",
        params![10i32, schema_str],
    )?;

    // Insert metric rows: 20 x u16 packed as 40-byte little-endian BLOB
    let tx = conn.unchecked_transaction()?;
    {
        let mut ins_metric = tx.prepare(
            "INSERT INTO metric (timestamp, schema_id, data) VALUES (?1, ?2, ?3)"
        )?;
        for (i, row) in rows.iter().enumerate() {
            let mut blob = Vec::with_capacity(40);
            for &val in row.iter() {
                blob.extend_from_slice(&val.to_le_bytes());
            }
            ins_metric.execute(params![timestamps[i], 10i32, blob])?;
        }
    }
    tx.commit()?;

    Ok(())
}
