//! CREATE DATABASE TEMPLATE whole-database-image control. This is not an
//! archive restore, pg_dump, power-loss, physical page-byte or economic proof.
use super::publication_process_tests::{native_read, read_document, write_one};
use super::*;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
fn quote(name: &str) -> String {
    assert!(
        !name.is_empty()
            && name.len() <= 63
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    );
    format!("\"{name}\"")
}
async fn zero_backends<C: GenericClient + Sync>(c: &C, name: &str) -> serde_json::Value {
    let end = Instant::now() + Duration::from_secs(2);
    let mut observations = Vec::new();
    loop {
        let rows=c.query("SELECT pid,backend_start::text,state FROM pg_stat_activity WHERE datname=$1 ORDER BY pid",&[&name]).await.unwrap();
        observations.push(json!(rows.iter().map(|r|json!({"pid":r.get::<_,i32>(0),"backend_start":r.get::<_,Option<String>>(1),"state":r.get::<_,Option<String>>(2)})).collect::<Vec<_>>()));
        if rows.is_empty() {
            return json!({"database":name,"observations":observations,"final_count":0});
        }
        assert!(
            Instant::now() < end,
            "source backends did not exit; retained image fixture"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
async fn inventory<C: GenericClient + Sync>(c: &C) -> serde_json::Value {
    let tables=c.query("SELECT schemaname,tablename FROM pg_tables WHERE schemaname NOT IN ('pg_catalog','information_schema') AND schemaname NOT LIKE 'pg_toast%' ORDER BY schemaname,tablename",&[]).await.unwrap();
    let mut data = BTreeMap::new();
    for row in tables {
        let schema: String = row.get(0);
        let name: String = row.get(1);
        let rows=c.query(&format!("SELECT encode(sha256(convert_to(row_to_json(t)::text,'UTF8')),'hex') FROM {}.{} t ORDER BY 1",quote(&schema),quote(&name)),&[]).await.unwrap();
        data.insert(
            format!("{schema}.{name}"),
            rows.iter()
                .map(|r| r.get::<_, String>(0))
                .collect::<Vec<_>>(),
        );
    }
    assert!(data.contains_key("ledgerlab.r3_commit_witness"));
    let columns=c.query("SELECT table_schema,table_name,column_name,data_type,is_nullable,COALESCE(column_default,'') FROM information_schema.columns WHERE table_schema NOT IN ('pg_catalog','information_schema') ORDER BY table_schema,table_name,ordinal_position",&[]).await.unwrap().iter().map(|r|(0..6).map(|n|r.get::<_,String>(n)).collect::<Vec<_>>()).collect::<Vec<_>>();
    let indexes=c.query("SELECT schemaname,tablename,indexname,indexdef FROM pg_indexes WHERE schemaname NOT IN ('pg_catalog','information_schema') AND schemaname NOT LIKE 'pg_toast%' ORDER BY schemaname,tablename,indexname",&[]).await.unwrap().iter().map(|r|(0..4).map(|n|r.get::<_,String>(n)).collect::<Vec<_>>()).collect::<Vec<_>>();
    let constraints=c.query("SELECT n.nspname,c.relname,k.conname,pg_get_constraintdef(k.oid) FROM pg_constraint k JOIN pg_class c ON c.oid=k.conrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname NOT IN ('pg_catalog','information_schema') ORDER BY n.nspname,c.relname,k.conname",&[]).await.unwrap().iter().map(|r|(0..4).map(|n|r.get::<_,String>(n)).collect::<Vec<_>>()).collect::<Vec<_>>();
    json!({"all_user_table_rows":data,"columns":columns,"indexes":indexes,"constraints":constraints})
}
fn anchor_bytes(path: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            (
                entry.file_name().to_str().unwrap().to_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL17/18 TLS; real CREATE DATABASE TEMPLATE image"]
async fn postgres_stale_whole_database_image_refused_by_current_anchor() {
    let root = PathBuf::from(
        std::env::var_os("LEDGERLAB_PG_CUT_EVIDENCE").expect("retained evidence directory"),
    );
    let dir = root.join(format!("stale-image-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let anchor = dir.join("anchor");
    fs::create_dir(&anchor).unwrap();
    let Fixture { name, owner, store } = Fixture::new().await;
    let stale = format!("{name}_stale");
    store.close().await;
    owner
        .client
        .execute(
            "UPDATE ledgerlab.installation SET admission='frozen' WHERE singleton=1",
            &[],
        )
        .await
        .unwrap();
    fs::write(dir.join("fixture.json"),serde_json::to_vec_pretty(&json!({"source":name,"stale_image":stale,"anchor":anchor,"copy_kind":"CREATE DATABASE TEMPLATE, whole database image control","physical_or_economic_claim":false})).unwrap()).unwrap();
    eprintln!(
        "PG_STALE_IMAGE source={name} stale={stale} anchor={}",
        anchor.display()
    );
    let publication = super::super::super::bootstrap::bind(config(&name, true), &anchor, "center")
        .await
        .unwrap();
    drop(publication);
    owner.discard().await;
    let admin = config("ledgerlab", true).connect().await.unwrap();
    let zero = zero_backends(&admin.client, &name).await;
    fs::write(
        dir.join("source-zero-before-copy.json"),
        serde_json::to_vec_pretty(&zero).unwrap(),
    )
    .unwrap();
    let copied_anchor = anchor_bytes(&anchor);
    admin
        .client
        .batch_execute(&format!(
            "CREATE DATABASE {} TEMPLATE {}",
            quote(&stale),
            quote(&name)
        ))
        .await
        .unwrap();
    let current = PostgresStore::open_fenced(config(&name, false), &anchor)
        .await
        .unwrap();
    write_one(&current, false).await;
    write_one(&current, true).await;
    current.close().await;
    let zero_after = zero_backends(&admin.client, &name).await;
    fs::write(
        dir.join("source-zero-after-commits.json"),
        serde_json::to_vec_pretty(&zero_after).unwrap(),
    )
    .unwrap();
    let latest_anchor = anchor_bytes(&anchor);
    assert_ne!(copied_anchor, latest_anchor);
    let stale_observer = config(&stale, true).connect().await.unwrap();
    let copied_record: serde_json::Value =
        serde_json::from_slice(&copied_anchor["state.json"]).unwrap();
    let actual_stale = super::super::publication::read_witness(&stale_observer.client)
        .await
        .unwrap();
    assert_eq!(
        actual_stale.anchor().as_str(),
        copied_record["anchor"].as_str().unwrap()
    );
    assert_eq!(
        actual_stale.witness().as_str(),
        copied_record["publication"]["witness"].as_str().unwrap()
    );
    let stale_before = inventory(&stale_observer.client).await;
    fs::write(
        dir.join("stale-inventory-before.json"),
        serde_json::to_vec_pretty(&stale_before).unwrap(),
    )
    .unwrap();
    let result = PostgresStore::open_fenced(config(&stale, false), &anchor).await;
    let error = match result {
        Err(error) => error,
        Ok(store) => {
            store.close().await;
            panic!("stale whole image accepted with current external owner")
        }
    };
    assert!(
        matches!(
            error,
            StoreError::InvalidStore("PostgreSQL trusted anchor requires authoritative recovery")
        ),
        "unexpected refusal boundary: {error:?}"
    );
    assert_eq!(
        anchor_bytes(&anchor),
        latest_anchor,
        "stale refusal rewrote external owner record"
    );
    let stale_after = inventory(&stale_observer.client).await;
    assert_eq!(
        stale_after, stale_before,
        "stale refusal changed retained rows or schema inventory"
    );
    fs::write(
        dir.join("stale-inventory-after.json"),
        serde_json::to_vec_pretty(&stale_after).unwrap(),
    )
    .unwrap();
    let source_observer = config(&name, true).connect().await.unwrap();
    let source_before = inventory(&source_observer.client).await;
    for _ in 0..2 {
        let latest = PostgresStore::open_fenced(config(&name, false), &anchor)
            .await
            .unwrap();
        read_document(&latest).await;
        native_read(&latest, true).await;
        latest.close().await;
        assert_eq!(anchor_bytes(&anchor), latest_anchor);
        assert_eq!(
            inventory(&source_observer.client).await,
            source_before,
            "latest saved retry/read/reopen changed inventory"
        );
    }
    fs::write(
        dir.join("source-latest-inventory.json"),
        serde_json::to_vec_pretty(&source_before).unwrap(),
    )
    .unwrap();
    fs::write(dir.join("result.json"),serde_json::to_vec_pretty(&json!({"status":"PASS","copy_kind":"CREATE DATABASE TEMPLATE","source_zero_before_copy":zero,"source_zero_after_commits":zero_after,"refusal":format!("{error:?}"),"stale_complete_row_schema_inventory_unchanged":true,"current_external_anchor_unchanged_on_refusal":true,"source_latest_exact_document_native_saved_reopen":true,"copied_anchor":copied_anchor,"latest_anchor":latest_anchor,"user_table_count":stale_before["all_user_table_rows"].as_object().unwrap().len()})).unwrap()).unwrap();
    stale_observer.discard().await;
    source_observer.discard().await;
    admin.discard().await;
    eprintln!("PG_STALE_IMAGE PASS retained={}", dir.display());
}
