//! Exact accepted customer95 chronology on five actual PostgreSQL journals.
//! Runtime/persistence evidence only: the parent's physical envelope is assumed.
use super::*;
use std::collections::BTreeSet;
#[path = "runtime_oracle.rs"]
mod runtime_oracle;
use runtime_oracle::CustomerOracle;
pub(super) fn proof_key(v: &Json) -> String {
    serde_json::to_string(&json!([v["host"], v["fact_kind"], v["full_key"]])).unwrap()
}
pub(super) fn hydrate(c: &mut Json, proofs: &BTreeMap<String, Json>, root: &Digest) {
    for field in ["proof", "begin"] {
        if c["payload"].get(field).is_some() {
            c["payload"][field] = proofs[&proof_key(&c["payload"][field])].clone();
        }
    }
    if let Some(list) = c["payload"]
        .get_mut("preparations")
        .and_then(Json::as_array_mut)
    {
        for p in list.iter_mut() {
            *p = proofs[&proof_key(p)].clone();
        }
        list.sort_by_cached_key(|p| r3::canonical_bytes(p, r3::COMMAND_BYTES).unwrap());
    }
    if c["kind"] == "LOCAL_GRANT" {
        c["payload"]["grant"]["journal_head"] = json!(root);
        let mut unsigned = c["payload"]["grant"].clone();
        unsigned.as_object_mut().unwrap().remove("authentication");
        c["payload"]["grant"]["authentication"] = json!(runtime::hash("grant", &unsigned).unwrap());
    }
    c["authority"]["head"] = json!(root);
    c["authority"]["command"] = json!(runtime::command_digest(parse(c).command()).unwrap());
}
pub(super) fn fact(c: &Json) -> Option<(&'static str, Json)> {
    let p = &c["payload"];
    Some(match c["kind"].as_str().unwrap() {
        "PREPARE_ENROLL" => ("ENROLL_PREPARATION", p["gateway"].clone()),
        "ENROLL" => ("ENROLLMENT", json!("registration")),
        "LOCAL_GRANT" => ("GRANT", p["grant"]["id"].clone()),
        "ISSUE" => ("CLAIM", p["token"]["id"].clone()),
        "RECEIVE" => ("RECEIPT", p["token"].clone()),
        "RETURN_UNUSED" => ("RETURNED_UNUSED", p["token"].clone()),
        "RECONCILE" => ("RECONCILIATION", p["token"].clone()),
        "BEGIN" => ("BEGIN", p["round"].clone()),
        "SEALED" => ("SEAL", p["round"].clone()),
        "CLOSE" => ("TERMINAL", p["round"].clone()),
        "INSTALL" => ("INSTALLATION", p["round"].clone()),
        _ => return None,
    })
}
#[tokio::test]
#[ignore = "requires isolated PG17/18; physical envelope ASSUMED TEST ONLY; no production admission"]
async fn native_runtime_exact_customer_ninety_five_commands() {
    let input:Json=serde_json::from_str(include_str!("../../../../../../contracts/candidates/central-adjudication-r3-candidate1/customer-trace.json")).unwrap();
    let base = BaseFixture::new(&input);
    let sources: Vec<wire::AuthoritySource> =
        serde_json::from_value(input["initial"]["authority_sources"].clone()).unwrap();
    let auth = sources
        .iter()
        .find(|s| {
            s.body_hash.as_str()
                == input["commands"][0]["authority"]["document"]
                    .as_str()
                    .unwrap()
        })
        .unwrap();
    let mut host = Host {
        stores: BTreeMap::new(),
        heads: BTreeMap::new(),
        sources: sources.clone(),
        base,
        now: serde_json::from_value(input["commands"][0]["authority"]["observed_at"].clone())
            .unwrap(),
        fault_count: Arc::new(AtomicU64::new(0)),
    };
    for name in ["center", "g0", "g1", "g2", "g3"] {
        let mut f = Fixture::new_scope("synthetic").await;
        let head = provision(&mut f, &journal(name), auth).await;
        if name == "center" {
            provision_original(&mut f, &host.base).await;
        }
        host.heads.insert(name.into(), head);
        host.stores.insert(name.into(), f);
    }
    let mut roots: BTreeMap<String, Digest> = host
        .stores
        .keys()
        .map(|h| (h.clone(), Digest::parse(&"0".repeat(64)).unwrap()))
        .collect();
    let mut ordinals: BTreeMap<String, u128> = roots.keys().map(|h| (h.clone(), 0)).collect();
    let mut proofs = BTreeMap::new();
    let mut last = BTreeMap::new();
    let mut kinds = BTreeSet::new();
    let mut oracle = CustomerOracle::default();
    let mut witness = vec![];
    let schedule = input["commands"].as_array().unwrap();
    assert_eq!(schedule.len(), 95);
    for (index, c) in schedule.iter().enumerate() {
        let mut c = c.clone();
        let kind = c["kind"].as_str().unwrap().to_owned();
        kinds.insert(kind.clone());
        let owner = match kind.as_str() {
            "PREPARE_ENROLL" | "ACTIVATE" | "RECEIVE" | "RETURN_UNUSED" | "LOCAL_TERMINAL"
            | "SEAL_BEGIN" | "SEALED" | "INSTALL" => c["payload"]["gateway"].as_str().unwrap(),
            "LOCAL_GRANT" => c["payload"]["grant"]["gateway"].as_str().unwrap(),
            _ => "center",
        }
        .to_owned();
        host.now = serde_json::from_value(c["authority"]["observed_at"].clone()).unwrap();
        hydrate(&mut c, &proofs, &roots[&owner]);
        if kind == "REGISTER_GRANT" {
            let p: wire::Proof = serde_json::from_value(c["payload"]["proof"].clone()).unwrap();
            let source = host
                .source(&SourceRequest::Exact(Box::new(p)))
                .await
                .unwrap();
            let body: Json = serde_json::from_slice(
                &r3::proofs::decode_base64(&source.object().body, r3::COMMAND_BYTES).unwrap(),
            )
            .unwrap();
            c["payload"]["grant"] = body["payload"]["grant"].clone();
            hydrate(&mut c, &proofs, &roots[&owner]);
        }
        if matches!(index, 91 | 94) {
            let before = all_inventory(&host.stores[&owner]).await;
            let mut rejected = c.clone();
            rejected["key"][2] = json!(format!("pg-economic-negative-{index}"));
            let code = if index == 91 {
                rejected["payload"]["signed_atoms"] = json!("101");
                "ADJUSTMENT_CAPACITY"
            } else {
                rejected["payload"]["expected_revision"] = json!("2");
                "REVISION"
            };
            hydrate(&mut rejected, &proofs, &roots[&owner]);
            let error = execute(&host, &owner, &rejected, None).await;
            assert!(
                matches!(&error,Err(ServiceError::Rejection(c)) if c==code),
                "expected{code}:{error:?}"
            );
            assert_eq!(before, all_inventory(&host.stores[&owner]).await);
        }
        let result = execute(&host, &owner, &c, None)
            .await
            .unwrap_or_else(|e| panic!("step{} {kind} {owner}: {e}", index + 1));
        assert_eq!(result.status, wire::CommandResultStatus::Committed);
        assert_eq!(result.code, kind);
        roots.insert(owner.clone(), result.root.clone());
        *ordinals.get_mut(&owner).unwrap() += 1;
        oracle.observe(&host, index + 1, &input, &result).await;
        if let Some((kind, key)) = fact(&c) {
            let source = host
                .export(
                    &journal(&owner),
                    Count::new(ordinals[&owner]).unwrap(),
                    serde_json::from_value(json!(kind)).unwrap(),
                    serde_json::from_value(key).unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(source.prefix().root(), &result.root);
            let p = serde_json::to_value(source.proof()).unwrap();
            proofs.insert(proof_key(&p), p);
        }
        let before = all_inventory(&host.stores[&owner]).await;
        let mut retry = c.clone();
        retry["authority"]["permission"] = json!("read");
        let saved = execute(&host, &owner, &retry, None).await.unwrap();
        assert_eq!(saved.status, wire::CommandResultStatus::Duplicate);
        assert_eq!(saved.root, result.root);
        assert_eq!(saved.effects, result.effects);
        assert_eq!(before, all_inventory(&host.stores[&owner]).await);
        last.insert(owner.clone(), retry);
        let key = r3::canonical_bytes(&c["key"], 4096).unwrap();
        let stored=host.stores[&owner].owner.client.query_one(
            "SELECT delivery,command FROM ledgerlab.r3_commands WHERE journal=$1 AND delivery_hash=sha256($2)",
            &[&journal_key(&journal(&owner)).unwrap(),&key]
        ).await.unwrap();
        assert_eq!(stored.get::<_, Vec<u8>>(0), key);
        let exact_command = stored.get::<_, Vec<u8>>(1);
        assert_eq!(exact_command, parse(&c).bytes());
        let point = json!({"through":index+1,"kind":kind,"host":owner,"ordinal":ordinals[&owner].to_string(),"root":result.root,"command":c,"command_sha256":r3::raw_sha256(&exact_command),"result":result});
        eprintln!("PG_CUSTOMER_STEP {point}");
        witness.push(point);
    }
    assert_eq!(oracle.checkpoints.len(), 15);
    assert_eq!(kinds.len(), 22);
    assert_eq!(ordinals.values().sum::<u128>(), 95);
    for owner in ["center", "g0", "g1", "g2", "g3"] {
        let f = host.stores.get_mut(owner).unwrap();
        f.store.clone().close().await;
        f.store = PostgresStore::open(config(&f.name, false)).await.unwrap();
        host.now = serde_json::from_value(last[owner]["authority"]["observed_at"].clone()).unwrap();
        let before = all_inventory(&host.stores[owner]).await;
        let retry = execute(&host, owner, &last[owner], None).await.unwrap();
        assert_eq!(retry.root, roots[owner]);
        assert_eq!(retry.status, wire::CommandResultStatus::Duplicate);
        assert_eq!(before, all_inventory(&host.stores[owner]).await);
    }
    let absent = [
        "ABORT",
        "EXTEND_RESOURCES",
        "PREPARE_ROUND",
        "REPLACE_WRITER",
        "RETIRE_GRANT",
        "RETURN_UNUSED",
        "SUPPLEMENT",
    ];
    assert!(absent.iter().all(|k| !kinds.contains(*k)));
    println!("PG_CUSTOMER_CHECKPOINTS {}", json!(oracle.checkpoints));
    println!("PG_CUSTOMER_WITNESS {}", json!(witness));
    println!(
        "PG_CUSTOMER_SUMMARY {}",
        json!({"commands":95,"exact_retries":95,"reopened_host_retries":5,"covered_kinds":kinds,"absent_kinds":absent,"heads":roots,"ordinals":ordinals,"physical_admission":"ASSUMED TEST ONLY / UNPROVED"})
    );
    for (_, f) in host.stores {
        f.finish().await;
    }
}
