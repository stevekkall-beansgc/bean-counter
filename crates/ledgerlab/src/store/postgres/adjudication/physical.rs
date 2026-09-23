//! Executable input worksheet and actual TOAST/page observations. These numbers
//! are SQL value payloads, NOT heap/index/TOAST/WAL allocation quotes. Exact
//! PostgreSQL source constants and enforced aggregate backing are still absent.
use super::*;
const J: u64 = r3::MAX_KEY_BYTES as u64;
const N: u64 = 16;
const HASH: u64 = 64;
const VALUE: u64 = r3::SEGMENT_BYTES as u64;
struct RowInput {
    table: &'static str,
    value_bytes: u64,
    index_key_bytes: &'static [u64],
}
// Adapter-validated input bounds, not bounds on arbitrary owner SQL. Some text
// and referenced columns rely on the typed adapter rather than local CHECKs.
// Include generated digests and repeated full identities. SQL tuple headers,
// varlena framing/alignment, page splits, TOAST chunks, FSM/VM, WAL and indexes
// on TOAST are intentionally not guessed from these application-level lengths.
const ROWS: &[RowInput] = &[
    RowInput {
        table: "r3_commit_witness",
        value_bytes: 2 + 2 * HASH,
        index_key_bytes: &[2],
    },
    RowInput {
        table: "r3_scope_locks",
        value_bytes: J + 2 + J,
        index_key_bytes: &[J + 2 + J],
    },
    RowInput {
        table: "r3_journals",
        value_bytes: J + 4096 + N + 2 * HASH,
        index_key_bytes: &[J],
    },
    RowInput {
        table: "r3_storage_profile",
        value_bytes: 4 + J + 8192 + 8192 + 2 * N,
        index_key_bytes: &[4, J],
    },
    RowInput {
        table: "r3_segments",
        value_bytes: J + N + 2 * HASH + 8,
        index_key_bytes: &[J + N, J + HASH],
    },
    RowInput {
        table: "r3_segment_pages",
        value_bytes: J + N + 4 + 4096,
        index_key_bytes: &[J + N + 4],
    },
    RowInput {
        table: "r3_objects",
        value_bytes: 64 + J + N + 32 + 2048 + 4096 + HASH + 4 + 8192,
        index_key_bytes: &[J + 32 + 32 + 32 + HASH, J + 32 + 32, J + 32 + HASH + N],
    },
    RowInput {
        table: "r3_object_pages",
        value_bytes: 64 + J + 2048 + 32 + 4096 + HASH + 4 + 4096,
        index_key_bytes: &[J + 32 + 32 + 32 + HASH + 4],
    },
    RowInput {
        table: "r3_heads",
        value_bytes: J + 4 + J + N + VALUE,
        index_key_bytes: &[J + 4 + J],
    },
    RowInput {
        table: "r3_head_versions",
        value_bytes: J + 4 + J + 2 * N + VALUE,
        index_key_bytes: &[J + 4 + J + N],
    },
    RowInput {
        table: "r3_commands",
        value_bytes: 32 + J + 4096 + N + 262144 + VALUE + 8192,
        index_key_bytes: &[J + 32, J + N],
    },
    RowInput {
        table: "r3_namespaces",
        value_bytes: 128 + 128 + 32 + 128 + J,
        index_key_bytes: &[128 + 128 + 32],
    },
    RowInput {
        table: "r3_deliveries",
        value_bytes: 128 + 128 + 256 + 128 + J + 16384,
        index_key_bytes: &[128 + 128 + 256 + 128],
    },
    RowInput {
        table: "r3_index_pages",
        value_bytes: J + HASH + 4096,
        index_key_bytes: &[J + HASH],
    },
    RowInput {
        table: "r3_index_roots",
        value_bytes: J + J + N + HASH,
        index_key_bytes: &[J + J + N],
    },
    RowInput {
        table: "r3_held_intentions",
        value_bytes: J + N + 4 + 8192 + 4,
        index_key_bytes: &[J + N + 4],
    },
    RowInput {
        table: "r3_unresolved_work",
        value_bytes: 4 + N + 9 + 4 + 8 + J + 4096 + HASH,
        index_key_bytes: &[4],
    },
];
#[test]
fn native_physical_input_worksheet_is_explicitly_not_an_allocation_quote() {
    assert_eq!(r3::SEGMENT_BYTES, 8_388_608);
    assert_eq!(r3::PAGE_BYTES, 4096);
    assert_eq!(ROWS.len(), 17);
    let mut names = std::collections::BTreeSet::new();
    for r in ROWS {
        assert!(names.insert(r.table));
        assert!(r.value_bytes > 0);
        assert!(!r.index_key_bytes.is_empty());
    }
    assert_eq!(
        ROWS.iter()
            .find(|r| r.table == "r3_heads")
            .unwrap()
            .value_bytes,
        8_390_858
    );
    assert_eq!(
        ROWS.iter()
            .find(|r| r.table == "r3_head_versions")
            .unwrap()
            .index_key_bytes,
        &[2250]
    );
    println!("SQL_VALUE_INPUT_WORKSHEET {}",serde_json::to_string(&ROWS.iter().map(|r|json!({"table":r.table,"max_value_payload_bytes":r.value_bytes,"index_key_payload_bytes":r.index_key_bytes})).collect::<Vec<_>>()).unwrap());
}
fn maximum_value() -> Vec<u8> {
    // Deterministic high-entropy textual data avoids crediting TOAST compression.
    // Actual stored sizes are measured below, never promoted to worst-case proof.
    let mut bytes = Vec::with_capacity(r3::SEGMENT_BYTES);
    bytes.extend(b"{\"x\":\"");
    let mut rng = 0x839928ac792abc91u64;
    while bytes.len() < r3::SEGMENT_BYTES - 2 {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        bytes.push(b"0123456789abcdef"[(rng & 15) as usize]);
    }
    bytes.extend(b"\"}");
    assert_eq!(bytes.len(), r3::SEGMENT_BYTES);
    bytes
}
#[tokio::test]
#[ignore = "requires explicit isolated PostgreSQL17/18; measurements are not physical upper bounds"]
async fn native_physical_maximum_heads_toast_and_resolve_budget() {
    let f = Fixture::new().await;
    let (j, s) = sample();
    let value = maximum_value();
    let command = runtime::command_value(&s.command).unwrap();
    let delivery = serde_json::from_value(command["key"].clone()).unwrap();
    let keys: Vec<_> = (0..3)
        .map(|n| {
            let mut key = vec![b'k'; r3::MAX_KEY_BYTES];
            key[0] = b'a' + n;
            HeadKey {
                journal: j.clone(),
                kind: HeadKind::Counter,
                full_key: key,
            }
        })
        .collect();
    let writes = keys
        .iter()
        .map(|k| HeadWrite {
            key: k.clone(),
            expected: None,
            revision: Count::new(1).unwrap(),
            value: value.clone(),
        })
        .collect();
    let wal: String = f
        .owner
        .client
        .query_one("SELECT pg_current_wal_insert_lsn()::text", &[])
        .await
        .unwrap()
        .get(0);
    let mut tx = begin(&f, &j).await;
    tx.native_adjudication(Operation::Primitive(Box::new(Primitive {
        journal: j.clone(),
        segment: s.clone(),
        writes,
        fail_after_segment: false,
    })))
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let mut tx = begin(&f, &j).await;
    let Value::Resolved(resolved) = tx
        .native_adjudication(Operation::Resolve(ResolveRequest {
            journal: j.clone(),
            key: delivery,
            guards: guards(&j),
            objects: vec![],
            heads: keys[..2].to_vec(),
        }))
        .await
        .unwrap()
    else {
        panic!("resolve")
    };
    assert_eq!(
        resolved
            .heads
            .iter()
            .map(|h| h.value.as_ref().unwrap().len())
            .sum::<usize>(),
        16_777_216
    );
    assert!(resolved
        .heads
        .iter()
        .all(|h| h.value.as_ref() == Some(&value)));
    tx.rollback().await.unwrap();
    let before = inventory(&f).await;
    let mut tx = begin(&f, &j).await;
    let key = serde_json::from_value(command["key"].clone()).unwrap();
    assert!(matches!(
        tx.native_adjudication(Operation::Resolve(ResolveRequest {
            journal: j.clone(),
            key,
            guards: guards(&j),
            objects: vec![],
            heads: keys
        }))
        .await,
        Err(StoreError::Overloaded)
    ));
    assert!(tx.commit().await.is_err());
    assert_eq!(inventory(&f).await, before);
    let rows=f.owner.client.query("SELECT c.relname,pg_relation_size(c.oid),pg_indexes_size(c.oid),pg_total_relation_size(c.oid),CASE WHEN c.reltoastrelid=0 THEN 0 ELSE pg_total_relation_size(c.reltoastrelid) END,pg_relation_size(c.oid,'fsm'),pg_relation_size(c.oid,'vm') FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='ledgerlab' AND c.relkind='r' ORDER BY c.relname",&[]).await.unwrap();
    let measured:Vec<_>=rows.iter().map(|r|json!({"table":r.get::<_,String>(0),"heap_main":r.get::<_,i64>(1),"indexes":r.get::<_,i64>(2),"total":r.get::<_,i64>(3),"toast_total":r.get::<_,i64>(4),"fsm":r.get::<_,i64>(5),"vm":r.get::<_,i64>(6)})).collect();
    for name in ["r3_heads", "r3_head_versions"] {
        let row = rows.iter().find(|r| r.get::<_, String>(0) == name).unwrap();
        assert!(row.get::<_, i64>(4) > 0, "TOAST must actually be exercised");
    }
    let delta: String = f
        .owner
        .client
        .query_one(
            "SELECT pg_wal_lsn_diff(pg_current_wal_insert_lsn(),$1::text::pg_lsn)::text",
            &[&wal],
        )
        .await
        .unwrap()
        .get(0);
    println!("OBSERVED_ALLOCATION_NOT_BOUND {}",serde_json::to_string(&json!({"relations":measured,"cluster_wal_delta_including_possible_other_activity":delta,"value_bytes":value.len(),"current_and_version_values":6})).unwrap());
    let indexes=f.owner.client.query("SELECT tablename,indexname,indexdef FROM pg_indexes WHERE schemaname='ledgerlab' ORDER BY tablename,indexname",&[]).await.unwrap();
    for row in ROWS {
        assert_eq!(
            indexes
                .iter()
                .filter(|i| i.get::<_, String>(0) == row.table)
                .count(),
            row.index_key_bytes.len(),
            "worksheet must account for every native index on {}",
            row.table
        );
    }
    let scalar = f.owner.client.query_one(
        "SELECT (SELECT typlen FROM pg_type WHERE oid='int2'::regtype), (SELECT typlen FROM pg_type WHERE oid='int4'::regtype), (SELECT typlen FROM pg_type WHERE oid='timestamptz'::regtype)", &[]
    ).await.unwrap();
    assert_eq!(
        (
            scalar.get::<_, i16>(0),
            scalar.get::<_, i16>(1),
            scalar.get::<_, i16>(2)
        ),
        (2, 4, 8)
    );
    println!("ACTUAL_INDEX_INVENTORY {}",serde_json::to_string(&indexes.iter().map(|r|json!({"table":r.get::<_,String>(0),"name":r.get::<_,String>(1),"definition":r.get::<_,String>(2)})).collect::<Vec<_>>()).unwrap());
    f.finish().await;
}
