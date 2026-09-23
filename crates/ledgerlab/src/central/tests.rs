use super::*;
use crate::store::outcomes::OutcomeStore;
use ledgerlab_core::canonical::outcome as codec;
fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../../contracts/candidates/central-adjudication-r3-candidate1/customer-trace.json"
    ))
    .unwrap()
}
fn context() -> HostContext {
    HostContext {
        principal: Id::parse("host").unwrap(),
        source: r3::types::Source::parse("urn:synthetic:work").unwrap(),
        observed_at: Time::parse("2026-09-22T00:00:00.000000Z").unwrap(),
    }
}
fn proposal(v: &Value) -> CommandProposal {
    CommandProposal::parse(
        &r3::canonical_bytes(
            &json!({"kind":v["kind"],"key":v["key"],"payload":v["payload"]}),
            r3::COMMAND_BYTES,
        )
        .unwrap(),
    )
    .unwrap()
}
fn records(v: &Value) -> Vec<Vec<u8>> {
    v["initial"]["original_objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r3::proofs::decode_base64(r["body"].as_str().unwrap(), r3::COMMAND_BYTES).unwrap())
        .collect()
}
struct Verifier {
    evidence: Vec<Vec<u8>>,
    target: String,
    deny: bool,
}
impl OriginalBaseVerifier for Verifier {
    fn verify(
        &self,
        c: OriginalVerificationContext<'_>,
        v: OriginalBaseView<'_>,
    ) -> Result<OriginalBaseAttestation, ServiceError> {
        require(
            !self.deny
                && c.principal == "host"
                && c.source == "urn:synthetic:work"
                && c.target == self.target
                && c.invocation_id == "invocation-supplier"
                && self.evidence.iter().all(|r| v.records().contains(r))
                && v.heads()
                    .iter()
                    .any(|h| h.class == "authority" && h.revision.is_some()),
            "TEST_HOST_VERIFIER",
        )?;
        let rows = self
            .evidence
            .iter()
            .map(|b| serde_json::from_slice::<Value>(b).unwrap())
            .collect::<Vec<_>>();
        Ok(OriginalBaseAttestation {
            authentication_document: rows
                .iter()
                .find(|r| r["body"]["purpose"] == "authentication")
                .unwrap()["body"]["document_id"]
                .as_str()
                .unwrap()
                .into(),
            verified_terms: rows
                .iter()
                .map(|r| r["body"]["document_id"].as_str().unwrap().into())
                .collect(),
            finality: true,
        })
    }
}
struct Fixture {
    host: LocalHost,
    configs: Vec<JournalConfig>,
    dirs: Vec<(tempfile::TempDir, tempfile::TempDir)>,
    sources: Vec<wire::AuthoritySource>,
    v: Value,
    base: OriginalBaseProposal,
    setup: OriginalBaseProvisioning,
    verifier: Arc<Verifier>,
}
impl Fixture {
    async fn new() -> Self {
        Self::with_extension(false).await
    }
    async fn with_extension(extend: bool) -> Self {
        let v = fixture();
        let raw = records(&v);
        let base = OriginalBaseProposal::from_records(raw.clone()).unwrap();
        let rows = raw
            .iter()
            .map(|r| serde_json::from_slice::<Value>(r).unwrap())
            .collect::<Vec<_>>();
        let evidence = rows
            .iter()
            .filter(|r| r["kind"] == "evidence")
            .map(|r| r3::canonical_bytes(r, r3::COMMAND_BYTES).unwrap())
            .collect::<Vec<_>>();
        let setup = OriginalBaseProvisioning {
            evidence: evidence.clone(),
            grant: OriginalAuthorityGrant {
                principal: Id::parse("host").unwrap(),
                source: context().source,
                grant_document: rows
                    .iter()
                    .find(|r| r["kind"] == "evidence" && r["body"]["purpose"] == "grant")
                    .unwrap()["body"]["document_id"]
                    .as_str()
                    .unwrap()
                    .into(),
                expected_revision: None,
                permissions: vec![OriginalPermission::Read, OriginalPermission::Submit],
            },
            bindings: rows
                .iter()
                .filter(|r| r["kind"] == "binding-snapshot")
                .map(|r| OriginalBinding {
                    binding_id: Id::parse(r["body"]["binding_id"].as_str().unwrap()).unwrap(),
                    reference: serde_json::from_value(codec::reference(r)).unwrap(),
                    expected_revision: None,
                })
                .collect(),
        };
        let verifier = Arc::new(Verifier {
            evidence,
            target: v["commands"][4]["payload"]["target"]
                .as_str()
                .unwrap()
                .into(),
            deny: false,
        });
        let sources: Vec<wire::AuthoritySource> =
            serde_json::from_value(v["initial"]["authority_sources"].clone()).unwrap();
        let mut dirs = Vec::new();
        let mut configs = Vec::new();
        let ws = Worksheet::frozen().unwrap();
        for host in ["center", "g0", "g1", "g2", "g3"] {
            let mut resources = wire::Resource::zero();
            for _ in 0..32 {
                for k in ws
                    .bundle(if host == "center" {
                        "finish_central"
                    } else {
                        "finish_gateway"
                    })
                    .unwrap()
                {
                    resources = resources
                        .checked_add(&ws.template(k).unwrap().resources().unwrap())
                        .unwrap();
                }
            }
            for _ in 0..8 {
                for t in ws.transitions.values() {
                    resources = resources.checked_add(&t.resources().unwrap()).unwrap();
                }
            }
            let db = tempfile::tempdir().unwrap();
            let anchor = tempfile::tempdir().unwrap();
            configs.push(JournalConfig {
                database: db.path().into(),
                anchor: anchor.path().into(),
                store: Id::parse("center").unwrap(),
                scope: serde_json::from_value(v["commands"][0]["key"][0].clone()).unwrap(),
                registration: Id::parse("registration").unwrap(),
                host: Id::parse(host).unwrap(),
                target: serde_json::from_value(v["commands"][4]["payload"]["target"].clone())
                    .unwrap(),
                resources: resources.clone(),
                ceiling: if extend && host == "g1" {
                    resources
                        .checked_add(&wire::Resource::from_dimensions(
                            [Count::new(1).unwrap(); 6],
                        ))
                        .unwrap()
                } else {
                    resources
                },
                legacy_pages: 65536,
                backing_bytes: Count::new(1 << 40).unwrap(),
                authority_source: Id::parse("synthetic-authority").unwrap(),
                authority_id: Id::parse("command-authority").unwrap(),
            });
            dirs.push((db, anchor));
        }
        let host = LocalHost::create_sandbox(
            configs.clone(),
            sources.clone(),
            Id::parse("operator").unwrap(),
            verifier.clone(),
        )
        .await
        .unwrap();
        for name in ["center", "g0", "g1", "g2", "g3"] {
            host.install_authority(&Id::parse(name).unwrap(), sources[0].clone(), None)
                .await
                .unwrap();
        }
        Self {
            host,
            configs,
            dirs,
            sources,
            v,
            base,
            setup,
            verifier,
        }
    }
    async fn prepare(&mut self) {
        let mut preparations = Vec::new();
        for n in 0..4 {
            let id = Id::parse(&format!("g{n}")).unwrap();
            let result = self
                .host
                .execute(
                    &id,
                    &proposal(&self.v["commands"][n]),
                    context(),
                    None,
                    Duration::from_secs(30),
                )
                .await
                .unwrap();
            assert_eq!(result.code, "PREPARE_ENROLL");
            preparations.push(
                serde_json::to_value(
                    self.host
                        .source_proof(
                            &id,
                            Count::new(1).unwrap(),
                            wire::FactKind::EnrollPreparation,
                            wire::ProofFullKey::V2(id.clone()),
                            context(),
                            Duration::from_secs(5),
                        )
                        .await
                        .unwrap(),
                )
                .unwrap(),
            );
        }
        preparations.sort_by_cached_key(|x| r3::canonical_bytes(x, r3::COMMAND_BYTES).unwrap());
        self.v["commands"][4]["payload"]["preparations"] = json!(preparations);
    }
    async fn enroll(&self) -> Result<wire::CommandResult, ServiceError> {
        self.host
            .execute(
                &Id::parse("center").unwrap(),
                &proposal(&self.v["commands"][4]),
                context(),
                Some(&self.base),
                Duration::from_secs(30),
            )
            .await
    }
    async fn inventory(&self) -> String {
        format!(
            "{:?}",
            self.host.entries["center"]
                .store
                .test_full_inventory()
                .await
        )
    }
}
#[tokio::test]
async fn public_fresh_atomic_enroll_proofs_retry_and_reopen() {
    let mut f = Fixture::new().await;
    f.host
        .provision_original(&Id::parse("center").unwrap(), f.setup.clone())
        .await
        .unwrap();
    f.prepare().await;
    let before = f.inventory().await;
    let result = f.enroll().await.unwrap();
    assert_eq!(result.code, "ENROLL");
    let e = &f.host.entries["center"];
    let command = f
        .base
        .command(&context(), &Id::parse("operator").unwrap())
        .unwrap();
    let raw: Vec<Value> = f
        .base
        .records()
        .iter()
        .map(|r| serde_json::from_slice(r).unwrap())
        .collect();
    let event = raw.iter().find(|r| r["kind"] == "event").unwrap();
    let locks = outcome::fresh_base_locks(&command).unwrap();
    let mut tx = e
        .store
        .begin_outcome(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    tx.lock_scopes(&locks).await.unwrap();
    let OutcomeResolution::Complete(snapshot) = tx
        .resolve_outcome(&OutcomeResolve {
            delivery: ScopedDelivery {
                scope: ["synthetic".into(), "sandbox".into()],
                source: context().source.as_str().into(),
                external_id: event["body"]["data"]["external_id"]
                    .as_str()
                    .unwrap()
                    .into(),
            },
            target: command.target.clone(),
            invocation_id: command.invocation_id.clone(),
            family_key: None,
            required: command.required.clone(),
            locks,
        })
        .await
        .unwrap()
    else {
        panic!("original snapshot")
    };
    assert_eq!(snapshot.anchors.len(), 1);
    assert_eq!(snapshot.records.len(), 29);
    for record in f.base.records() {
        assert!(snapshot.records.contains(record));
    }
    tx.rollback().await.unwrap();
    assert_ne!(f.inventory().await, before);
    let after = f.inventory().await;
    let retry = f
        .host
        .execute(
            &Id::parse("center").unwrap(),
            &proposal(&f.v["commands"][4]),
            context(),
            None,
            Duration::from_secs(30),
        )
        .await
        .unwrap();
    assert_eq!(retry.status, wire::CommandResultStatus::Duplicate);
    assert_eq!(retry.root, result.root);
    assert_eq!(f.inventory().await, after);
    let source = f
        .host
        .source_proof(
            &Id::parse("center").unwrap(),
            Count::new(1).unwrap(),
            wire::FactKind::Enrollment,
            wire::ProofFullKey::V2(Id::parse("registration").unwrap()),
            context(),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_eq!(source.root, result.root);
    let stats = f.host.entries["center"]
        .store
        .test_adjudication_stats(&f.host.entries["center"].journal)
        .await;
    assert_eq!(stats["segments"], 1);
    f.host.close().await;
    let reopened = LocalHost::open(
        f.configs,
        f.sources,
        Id::parse("operator").unwrap(),
        f.verifier,
    )
    .await
    .unwrap();
    let retry = reopened
        .execute(
            &Id::parse("center").unwrap(),
            &proposal(&f.v["commands"][4]),
            context(),
            None,
            Duration::from_secs(30),
        )
        .await
        .unwrap();
    assert_eq!(retry.root, result.root);
    assert_eq!(retry.status, wire::CommandResultStatus::Duplicate);
    reopened.close().await;
    drop(f.dirs);
}
#[tokio::test]
async fn public_host_rejects_missing_rights_principal_and_unconfigured_sources() {
    let mut f = Fixture::new().await;
    f.setup.grant.permissions = vec![OriginalPermission::Read];
    f.host
        .provision_original(&Id::parse("center").unwrap(), f.setup.clone())
        .await
        .unwrap();
    f.prepare().await;
    let before = f.inventory().await;
    assert!(
        matches!(f.enroll().await,Err(ServiceError::Rejection(c)) if c=="ORIGINAL_GRANT_AUTHORIZATION")
    );
    assert_eq!(f.inventory().await, before);
    let mut c = context();
    c.principal = Id::parse("attacker").unwrap();
    assert!(
        matches!(f.host.execute(&Id::parse("center").unwrap(),&proposal(&f.v["commands"][4]),c,Some(&f.base),Duration::from_secs(30)).await,Err(ServiceError::Rejection(c)) if c=="HOST_PRINCIPAL")
    );
    assert_eq!(f.inventory().await, before);
    let mut command = f.v["commands"][4].clone();
    command["payload"]["preparations"][0]["host"] = json!("unconfigured");
    assert!(
        matches!(f.host.execute(&Id::parse("center").unwrap(),&proposal(&command),context(),Some(&f.base),Duration::from_secs(30)).await,Err(ServiceError::Rejection(c)) if c=="SOURCE_HOST_UNCONFIGURED")
    );
    assert_eq!(f.inventory().await, before);
    f.host.close().await;
}

#[tokio::test]
async fn public_enrollment_rolls_back_original_writes_and_uses_current_retry_rights() {
    let mut f = Fixture::new().await;
    f.host
        .provision_original(&Id::parse("center").unwrap(), f.setup.clone())
        .await
        .unwrap();
    f.prepare().await;
    let before = f.inventory().await;
    let gateway = Id::parse("g1").unwrap();
    let fence = f
        .host
        .writer_fence(&gateway, context(), Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(
        fence,
        f.host
            .writer_fence(&gateway, context(), Duration::from_secs(5))
            .await
            .unwrap()
    );
    let mut wrong = context();
    wrong.principal = Id::parse("attacker").unwrap();
    assert!(f
        .host
        .writer_fence(&gateway, wrong, Duration::from_secs(5))
        .await
        .is_err());
    assert_eq!(f.inventory().await, before);
    f.host.entries["center"]
        .store
        .test_fail_after_original_base();
    assert!(f.enroll().await.is_err());
    assert_eq!(
        f.inventory().await,
        before,
        "original records/target/receipt and R3 segment must roll back together"
    );
    let result = f.enroll().await.unwrap();
    assert_eq!(result.code, "ENROLL");
    let accepted = f.inventory().await;
    let mut reused = f.v["commands"][4].clone();
    reused["key"][2] = json!("second-enrollment");
    assert!(f
        .host
        .execute(
            &Id::parse("center").unwrap(),
            &proposal(&reused),
            context(),
            Some(&f.base),
            Duration::from_secs(30)
        )
        .await
        .is_err());
    assert_eq!(f.inventory().await, accepted);
    let mut source = f.sources[0].clone();
    let raw = r3::proofs::decode_base64(&source.body, 16384).unwrap();
    let mut body: Value = serde_json::from_slice(&raw).unwrap();
    body["revision"] = json!("2");
    body["permissions"]
        .as_array_mut()
        .unwrap()
        .retain(|p| p != "read");
    let raw = r3::canonical_bytes(&body, 16384).unwrap();
    source.body = original::b64(&raw);
    source.body_hash = r3::raw_sha256(&raw);
    source.bytes = Count::new(raw.len() as u128).unwrap();
    f.host
        .install_authority(
            &Id::parse("center").unwrap(),
            source,
            Some(Count::new(1).unwrap()),
        )
        .await
        .unwrap();
    let before = f.inventory().await;
    assert!(
        matches!(f.host.execute(&Id::parse("center").unwrap(),&proposal(&f.v["commands"][4]),context(),None,Duration::from_secs(30)).await,Err(ServiceError::Rejection(code)) if code=="HOST_READ_AUTHORITY")
    );
    assert_eq!(
        f.inventory().await,
        before,
        "saved outcomes still require current read rights"
    );
    f.host.close().await;
}

#[tokio::test]
async fn public_original_grant_binding_and_verifier_rejections_are_atomic() {
    for attack in ["grant", "revision", "verifier", "legacy-head"] {
        let mut f = Fixture::new().await;
        f.host
            .provision_original(&Id::parse("center").unwrap(), f.setup.clone())
            .await
            .unwrap();
        f.prepare().await;
        if attack == "verifier" {
            f.host.close().await;
            f.host = LocalHost::open(
                f.configs.clone(),
                f.sources.clone(),
                Id::parse("operator").unwrap(),
                Arc::new(Verifier {
                    evidence: f.verifier.evidence.clone(),
                    target: f.verifier.target.clone(),
                    deny: true,
                }),
            )
            .await
            .unwrap();
        } else {
            // Test-only adversarial trusted setup. No public request can supply
            // these head bytes, and no accepted/base row is inserted.
            let e = &f.host.entries["center"];
            let c = f
                .base
                .command(&context(), &Id::parse("operator").unwrap())
                .unwrap();
            let lock = outcome::fresh_base_locks(&c)
                .unwrap()
                .into_iter()
                .find(|l| l.class == OutcomeLockClass::Authority)
                .unwrap();
            let grant: Value = serde_json::from_slice(
                f.setup
                    .evidence
                    .iter()
                    .find(|r| {
                        serde_json::from_slice::<Value>(r).unwrap()["body"]["purpose"] == "grant"
                    })
                    .unwrap(),
            )
            .unwrap();
            let reference = codec::reference(&grant);
            let mut head = json!({"active":true,"grant":reference,"authorization":{"principal":"host","scope":e.journal.scope,"source":"urn:synthetic:work","grant":reference,"grant_revision":"2","permissions":["read","submit"]}});
            match attack {
                "grant" => {
                    head["authorization"]["grant"]["content_hash"] =
                        json!(format!("sha256:{}", "0".repeat(64)))
                }
                "revision" => head["authorization"]["grant_revision"] = json!("1"),
                "legacy-head" => {
                    head.as_object_mut().unwrap().remove("authorization");
                }
                _ => unreachable!(),
            }
            let mut lock = lock;
            lock.mode = OutcomeLockMode::Write;
            e.store
                .provision_original_authority(
                    &[],
                    &[(
                        lock,
                        Some("1".into()),
                        r3::canonical_bytes(&head, 16384).unwrap(),
                    )],
                )
                .await
                .unwrap();
        }
        let before = f.inventory().await;
        assert!(
            matches!(f.enroll().await,Err(ServiceError::Rejection(code)) if code==if attack=="verifier"{"TEST_HOST_VERIFIER"}else{"ORIGINAL_GRANT_AUTHORIZATION"}),
            "{attack}"
        );
        assert_eq!(f.inventory().await, before, "{attack}");
        f.host.close().await;
    }
}
#[tokio::test]
async fn public_original_setup_rejects_economic_records_and_failed_cas_without_growth() {
    let f = Fixture::new().await;
    let before = f.inventory().await;
    let mut setup = f.setup.clone();
    setup.evidence.push(
        f.base
            .records()
            .iter()
            .find(|r| serde_json::from_slice::<Value>(r).unwrap()["kind"] == "base-acceptance")
            .unwrap()
            .clone(),
    );
    assert!(f
        .host
        .provision_original(&Id::parse("center").unwrap(), setup)
        .await
        .is_err());
    assert_eq!(f.inventory().await, before);
    f.host
        .provision_original(&Id::parse("center").unwrap(), f.setup.clone())
        .await
        .unwrap();
    let before = f.inventory().await;
    assert!(f
        .host
        .provision_original(&Id::parse("center").unwrap(), f.setup.clone())
        .await
        .is_err());
    assert_eq!(f.inventory().await, before);
    f.host.close().await;
}
#[test]
fn request_has_no_authority_or_verifier_override_and_permission_map_is_exact() {
    let v = fixture();
    assert!(CommandProposal::parse(
        &r3::canonical_bytes(&v["commands"][4], r3::COMMAND_BYTES).unwrap()
    )
    .is_err());
    for (kind, want) in [
        ("ENROLL", "enroll"),
        ("SUPPLEMENT", "submit"),
        ("ABORT", "close"),
        ("REPLACE_WRITER", "replace"),
        ("DECIDE", "decide"),
        ("CORRECT", "correct"),
        ("ISSUE", "capacity"),
    ] {
        assert_eq!(permission(&json!({"kind":kind,"payload":{}}), false), want);
        assert_eq!(permission(&json!({"kind":kind}), true), "read");
    }
    assert_eq!(
        permission(
            &json!({"kind":"DECIDE","payload":{"path":"ADJUSTMENT"}}),
            false
        ),
        "adjust"
    );
}

#[tokio::test]
async fn public_proof_and_fence_reads_bind_current_target_before_and_after_enrollment() {
    let mut f = Fixture::new().await;
    f.host
        .provision_original(&Id::parse("center").unwrap(), f.setup.clone())
        .await
        .unwrap();
    f.prepare().await;
    f.enroll().await.unwrap();
    for name in ["g1", "center"] {
        let id = Id::parse(name).unwrap();
        let mut source = f.sources[0].clone();
        let mut body: Value =
            serde_json::from_slice(&r3::proofs::decode_base64(&source.body, 16384).unwrap())
                .unwrap();
        body["revision"] = json!("2");
        body["target"] = json!("wrong-target");
        let raw = r3::canonical_bytes(&body, 16384).unwrap();
        source.body = original::b64(&raw);
        source.body_hash = r3::raw_sha256(&raw);
        source.bytes = Count::new(raw.len() as u128).unwrap();
        f.host
            .install_authority(&id, source, Some(Count::new(1).unwrap()))
            .await
            .unwrap();
        let before = format!(
            "{:?}",
            f.host.entries[name].store.test_full_inventory().await
        );
        let (kind, key) = if name == "center" {
            (
                wire::FactKind::Enrollment,
                Id::parse("registration").unwrap(),
            )
        } else {
            (wire::FactKind::EnrollPreparation, id.clone())
        };
        assert!(
            matches!(f.host.source_proof(&id, Count::new(1).unwrap(), kind, wire::ProofFullKey::V2(key), context(), Duration::from_secs(5)).await, Err(ServiceError::Rejection(c)) if c == "HOST_READ_AUTHORITY")
        );
        assert!(
            matches!(f.host.writer_fence(&id, context(), Duration::from_secs(5)).await, Err(ServiceError::Rejection(c)) if c == "HOST_READ_AUTHORITY")
        );
        assert_eq!(
            format!(
                "{:?}",
                f.host.entries[name].store.test_full_inventory().await
            ),
            before
        );
    }
    // A trusted but incorrect reopened target cannot override persisted enrollment.
    f.host.close().await;
    for c in &mut f.configs {
        c.target = Id::parse("wrong-target").unwrap();
    }
    f.host = LocalHost::open(
        f.configs.clone(),
        f.sources.clone(),
        Id::parse("operator").unwrap(),
        f.verifier.clone(),
    )
    .await
    .unwrap();
    assert!(
        matches!(f.host.writer_fence(&Id::parse("center").unwrap(), context(), Duration::from_secs(5)).await, Err(ServiceError::Rejection(c)) if c == "HOST_TARGET")
    );
    f.host.close().await;
}

#[path = "public_paths.rs"]
mod public_paths;
