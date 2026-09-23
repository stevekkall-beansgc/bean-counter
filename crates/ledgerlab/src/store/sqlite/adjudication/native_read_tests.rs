//! Audit the actual SQL text used by the paged reader against the pinned engine.
use super::*;
use crate::store::sqlite::{tests::installation, SqliteStore};

type Instruction = (i64, String, i64, i64, i64, Option<String>, i64);

#[tokio::test]
async fn r3_native_read_programs_use_bounded_index_seeks() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::create(dir.path(), installation())
        .await
        .unwrap();
    let mut c = store.inner.readers.acquire().await.unwrap();
    let mut tested = std::collections::BTreeSet::new();
    for source in [
        include_str!("../adjudication.rs"),
        include_str!("reader.rs"),
        include_str!("../read.rs"),
    ] {
        for sql in source.split('"').filter(|s| {
            s.starts_with("SELECT ")
                && (s.contains(" INDEXED BY ")
                    || s.contains("octet_length(tenant)")
                    || s.starts_with("SELECT tenant,environment,logical_store_id")
                    || s.starts_with("SELECT profile,maximum_pages"))
        }) {
            if !tested.insert(sql) {
                continue;
            }
            let mut query = sqlx::query(sqlx::AssertSqlSafe(format!("EXPLAIN QUERY PLAN {sql}")));
            for _ in 0..sql.matches('?').count() {
                query = query.bind(0i64);
            }
            let rows = query.fetch_all(&mut *c).await.unwrap();
            let details: Vec<String> = rows.iter().map(|r| r.get("detail")).collect();
            assert!(
                details.iter().all(|d| d.starts_with("SEARCH ")
                    && !d.contains("AUTOMATIC")
                    && !d.contains("TEMP B-TREE")),
                "{sql}: {details:?}"
            );
            let mut query = sqlx::query(sqlx::AssertSqlSafe(format!("EXPLAIN {sql}")));
            for _ in 0..sql.matches('?').count() {
                query = query.bind(0i64);
            }
            let rows = query.fetch_all(&mut *c).await.unwrap();
            let program: Vec<Instruction> = rows
                .iter()
                .map(|r| {
                    (
                        r.get(0),
                        r.get(1),
                        r.get(2),
                        r.get(3),
                        r.get(4),
                        r.get(5),
                        r.get(6),
                    )
                })
                .collect();
            assert!(
                !program.iter().any(|r| matches!(
                    r.1.as_str(),
                    "Rewind"
                        | "Sort"
                        | "SorterSort"
                        | "SorterOpen"
                        | "OpenAutoindex"
                        | "OpenEphemeral"
                )),
                "unbounded program: {sql}: {program:?}"
            );
            assert!(
                program
                    .iter()
                    .any(|r| matches!(r.1.as_str(), "SeekGE" | "SeekLE" | "SeekRowid")),
                "no indexed seek: {sql}"
            );
            assert!(
                program.iter().filter(|r| r.1 == "Column").count() <= 8,
                "column bound: {sql}"
            );
            assert!(
                program
                    .iter()
                    .filter(|r| matches!(r.1.as_str(), "SeekGE" | "SeekLE" | "SeekRowid"))
                    .count()
                    <= 2,
                "seek bound: {sql}"
            );
            if program
                .iter()
                .any(|r| matches!(r.1.as_str(), "Next" | "Prev"))
            {
                assert!(
                    sql.ends_with("LIMIT 1") && program.iter().any(|r| r.1 == "DecrJumpZero"),
                    "unbounded range loop: {sql}"
                );
            }
            if sql.contains("length(") {
                assert!(
                    program.iter().any(|r| r.1 == "Column" && r.6 & 0x40 != 0),
                    "missing metadata-only length flag: {sql}"
                );
            }
            if sql.contains("r3_family_first_closure") {
                assert!(
                    !program
                        .iter()
                        .any(|r| r.5.as_deref().is_some_and(|v| v.contains("json_extract"))),
                    "closure index must avoid runtime JSON filtering"
                );
            }
            eprintln!("native-reader SQL {sql}\nplan {details:?}\nprogram {program:?}");
        }
    }
    assert!(
        tested.len() >= 18,
        "missing native query coverage: {}",
        tested.len()
    );
    drop(c);
    store.close().await;
}
