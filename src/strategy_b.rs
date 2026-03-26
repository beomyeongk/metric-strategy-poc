use rusqlite::{Connection, params};
use crate::metrics::METRIC_NAMES;

/// Strategy B: First Normal Form (1NF) + name_map
/// Schema:
///   name_map(id INTEGER PK, name TEXT)
///   metric(timestamp INTEGER, name_id INTEGER, value INTEGER)
pub fn run(rows: &[[u16; 20]], timestamps: &[i64]) -> rusqlite::Result<()> {
    let conn = Connection::open("b.db")?;

    conn.execute_batch("
        PRAGMA journal_mode=WAL;
        DROP TABLE IF EXISTS metric;
        DROP TABLE IF EXISTS name_map;
        CREATE TABLE name_map (
            id   INTEGER PRIMARY KEY,
            name TEXT    NOT NULL
        );
        CREATE TABLE metric (
            timestamp INTEGER NOT NULL,
            name_id   INTEGER NOT NULL,
            value     INTEGER NOT NULL
        );
    ")?;

    let tx = conn.unchecked_transaction()?;
    {
        // Insert name_map entries (id: 10~29)
        let mut ins_name = tx.prepare(
            "INSERT INTO name_map (id, name) VALUES (?1, ?2)"
        )?;
        for (i, &name) in METRIC_NAMES.iter().enumerate() {
            ins_name.execute(params![(10 + i) as i32, name])?;
        }
        drop(ins_name);

        // Insert metric rows: 20 INSERT per timestamp -> 200,000 total rows
        let mut ins_metric = tx.prepare(
            "INSERT INTO metric (timestamp, name_id, value) VALUES (?1, ?2, ?3)"
        )?;
        for (i, row) in rows.iter().enumerate() {
            for (j, &val) in row.iter().enumerate() {
                ins_metric.execute(params![timestamps[i], (10 + j) as i32, val as i32])?;
            }
        }
    }
    tx.commit()?;

    Ok(())
}
