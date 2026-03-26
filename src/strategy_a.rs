use rusqlite::{Connection, params};
use serde_json::{Map, Value};
use crate::metrics::METRIC_NAMES;

/// Strategy A: Native JSONB (BLOB) storage
/// Schema: metric(timestamp INTEGER, data BLOB)
pub fn run(rows: &[[u16; 20]], timestamps: &[i64]) -> rusqlite::Result<()> {
    let conn = Connection::open("a.db")?;

    conn.execute_batch("
        PRAGMA journal_mode=WAL;
        DROP TABLE IF EXISTS metric;
        CREATE TABLE metric (
            timestamp INTEGER NOT NULL,
            data      BLOB    NOT NULL  -- SQLite native JSONB (binary format)
        );
    ")?;

    let tx = conn.unchecked_transaction()?;
    {
        // jsonb(?2): converts JSON text to SQLite internal binary JSONB format before storing
        let mut stmt = tx.prepare(
            "INSERT INTO metric (timestamp, data) VALUES (?1, jsonb(?2))"
        )?;
        for (i, row) in rows.iter().enumerate() {
            let mut map = Map::new();
            for (j, &val) in row.iter().enumerate() {
                map.insert(METRIC_NAMES[j].to_string(), Value::Number(val.into()));
            }
            let json = serde_json::to_string(&map).unwrap();
            stmt.execute(params![timestamps[i], json])?;
        }
    }
    tx.commit()?;

    Ok(())
}
