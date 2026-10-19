//! Opt-in actual local financial service tests. Run from a captured test build
//! with LEDGER_WORKFLOW_TEST_FIXTURE naming a quiescent maintained synthetic
//! runner output (RUN.json, main-work.request.json, installation/).
//! Only a private temporary copy is opened; no raw financial append is used.
use super::*;
use sqlx::{Connection,SqliteConnection,sqlite::SqliteConnectOptions,AssertSqlSafe};
use std::{fs,path::{Path,PathBuf}};

fn copy_private_tree(source:&Path,destination:&Path){
    let metadata=fs::symlink_metadata(source).unwrap();
    assert!(!metadata.file_type().is_symlink(),"synthetic fixture must not contain symlinks");
    if metadata.is_dir(){
        fs::create_dir(destination).unwrap();
        fs::set_permissions(destination,metadata.permissions()).unwrap();
        for entry in fs::read_dir(source).unwrap(){let entry=entry.unwrap();copy_private_tree(&entry.path(),&destination.join(entry.file_name()));}
    }else{
        assert!(metadata.is_file(),"regular synthetic fixture file required");
        fs::copy(source,destination).unwrap();
        fs::set_permissions(destination,metadata.permissions()).unwrap();
    }
}
fn fixture(case:&str)->(tempfile::TempDir,PathBuf,Vec<u8>,OperationKey,Vec<u8>){
    assert!(crate::build_identity::embedded_inputs().is_ok(),"requires captured build inputs");
    let root=PathBuf::from(std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE").expect("explicit maintained synthetic fixture required"));
    let run:Value=serde_json::from_slice(&fs::read(root.join("RUN.json")).unwrap()).unwrap();
    assert!(run["classification"].as_str().unwrap().to_ascii_lowercase().contains("synthetic"));
    let temporary=tempfile::tempdir().unwrap();
    let path=temporary.path().join("installation");
    copy_private_tree(&root.join("installation"),&path);
    let config=fs::read(path.join("billing.json")).unwrap();
    let mut request:Value=serde_json::from_slice(&fs::read(root.join("main-work.request.json")).unwrap()).unwrap();
    assert_eq!(request["kind"],"work.accept");
    assert_eq!(request["payload"]["schema"],"ledger-workflow-work/1");
    let tag=format!("local10-{case}-{}",temporary.path().file_name().unwrap().to_str().unwrap());
    for field in ["external_event_id","business_operation_id","chain"]{request["payload"][field]=json!(tag);}
    request["key"]["operation_id"]=json!(tag);
    request["payload"]["occurred_at"]=json!(crate::local::now().unwrap().as_str());
    let key=OperationKey::parse(&records::bytes(&request["key"]).unwrap()).unwrap();
    (temporary,path,records::bytes(&request).unwrap(),key,config)
}
/// Read-only equality of retained workflow bytes and all workflow projections.
/// This is test observation, never input to financial construction.
async fn snapshot(path:&Path)->Vec<(String,Vec<String>)>{
    let options=SqliteConnectOptions::new().filename(path.join(".ledger/local.db")).read_only(true);
    let mut connection=SqliteConnection::connect_with(&options).await.unwrap();
    let tables:Vec<String>=sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type='table' AND name LIKE 'workflow_%' ORDER BY name").fetch_all(&mut connection).await.unwrap();
    let mut result=Vec::new();
    for table in tables{
        let columns:Vec<String>=sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid").bind(&table).fetch_all(&mut connection).await.unwrap();
        let quoted=columns.iter().map(|column|format!("quote(\"{}\")",column.replace('"',"\"\""))).collect::<Vec<_>>().join("||'|'||");
        let query=format!("SELECT {quoted} FROM \"{}\" ORDER BY 1",table.replace('"',"\"\""));
        let rows:Vec<String>=sqlx::query_scalar(AssertSqlSafe(query)).fetch_all(&mut connection).await.unwrap();
        result.push((table,rows));
    }
    connection.close().await.unwrap();result
}
fn operations(snapshot:&[(String,Vec<String>)])->usize{
    snapshot.iter().find(|(table,_)|table=="workflow_operations").unwrap().1.len()
}

#[tokio::test]
#[ignore="requires captured build and explicit quiescent maintained synthetic fixture"]
async fn local10_precommit_cut_leaves_no_partial_financial_append(){
    let (_temporary,path,raw,key,config)=fixture("precommit");
    let before=snapshot(&path).await;
    let ledger=LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.store.test_commit_cut(1);
    assert!(matches!(ledger.financial(&raw).await,Err(ServiceError::OutcomeUnknown{..})));
    ledger.close().await;
    assert_eq!(snapshot(&path).await,before,"operations, documents, heads and claims must all roll back");
    assert_eq!(fs::read(path.join("billing.json")).unwrap(),config);
    let reopened=LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(reopened.status(&key.bytes().unwrap()).await.unwrap()["status"],"not_observed");
    reopened.close().await;
    assert_eq!(snapshot(&path).await,before);
}

#[tokio::test]
#[ignore="requires captured build and explicit quiescent maintained synthetic fixture"]
async fn local10_committed_reply_loss_reopens_and_identity_retries_once(){
    let (_temporary,path,raw,key,config)=fixture("reply-loss");
    let before=snapshot(&path).await;
    let ledger=LocalWorkflowLedger::open_local(&path).await.unwrap();
    inject_committed_reply_loss(key.bytes().unwrap());
    assert_eq!(ledger.financial(&raw).await.unwrap_err(),ServiceError::ResponseLost);
    ledger.close().await;
    let committed=snapshot(&path).await;
    assert_eq!(operations(&committed),operations(&before)+1);
    assert_eq!(fs::read(path.join("billing.json")).unwrap(),config);
    let reopened=LocalWorkflowLedger::open_local(&path).await.unwrap();
    let original=reopened.status(&key.bytes().unwrap()).await.unwrap();
    assert_eq!(original["status"],"committed");
    assert_eq!(original["receipt"]["key"],serde_json::to_value(&key).unwrap());
    let retry=reopened.financial(&raw).await.unwrap();
    assert_eq!(retry["duplicate_kind"],"identity");
    assert_eq!(retry["receipt"],original["receipt"]);
    reopened.close().await;
    assert_eq!(snapshot(&path).await,committed,"status and identity retry cannot append or mutate retained workflow data");
    assert_eq!(fs::read(path.join("billing.json")).unwrap(),config);
}
