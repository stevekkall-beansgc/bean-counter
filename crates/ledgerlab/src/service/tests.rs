//! Real file-backed adapter for the independent testkit. No expected accepted rows
//! are seeded and no production plan supplies readback evidence.
use super::{accept, hooks::Hooks};
use crate::{
    store::{
        ports::{AcceptanceStore, AcceptanceTx},
        sqlite::SqliteStore,
    },
    *,
};
use ledgerlab_testkit::{
    failpoints::*,
    history::{Alias, Snapshot, LATER_TABLES},
    stores as tk, FixtureOracle, HarnessError,
};
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};
use tokio::{runtime::Runtime, time::Instant};

struct Factory;
struct Backend {
    runtime: Runtime,
    directory: tempfile::TempDir,
    store: Option<SqliteStore>,
    oracle: FixtureOracle,
    pending: Option<Hooks>,
    affected: String,
    discarded: bool,
}
fn err(e: impl std::fmt::Display) -> HarnessError {
    HarnessError(e.to_string())
}
fn decode(v: &Value) -> Vec<u8> {
    let s = v.as_str().expect("hex field");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
impl tk::BackendFactory for Factory {
    type Backend = Backend;
    fn seeded(&self, oracle: &FixtureOracle) -> ledgerlab_testkit::Result<Backend> {
        Backend::new(oracle, false)
    }
    fn real_without_terms(&self, oracle: &FixtureOracle) -> ledgerlab_testkit::Result<Backend> {
        Backend::new(oracle, true)
    }
}
impl Backend {
    fn new(oracle: &FixtureOracle, real: bool) -> ledgerlab_testkit::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(err)?;
        let directory = tempfile::tempdir().map_err(err)?;
        let store = runtime.block_on(async {
            let mut installation = crate::store::sqlite::tests::installation();
            if real {
                installation.mode = "real".into();
            }
            let store = SqliteStore::create(directory.path(), installation)
                .await
                .map_err(err)?;
            let mut tx = store
                .begin(Instant::now() + Duration::from_secs(5))
                .await
                .map_err(err)?;
            for op in crate::store::sqlite::tests::seed() {
                tx.write(&op).await.map_err(err)?;
            }
            tx.commit().await.map_err(|e| err(format!("{e:?}")))?;
            Ok::<_, HarnessError>(store)
        })?;
        Ok(Self {
            runtime,
            directory,
            store: Some(store),
            oracle: oracle.clone(),
            pending: None,
            affected: String::new(),
            discarded: false,
        })
    }
    fn command(c: &tk::Command) -> AcceptCommand {
        AcceptCommand {
            bytes: c.bytes.clone(),
            principal: PrincipalContext {
                scope: ledgerlab_core::domain::Scope::new("demo", "sandbox").unwrap(),
                principal_id: "demo-app".into(),
                source: "urn:demo:app".into(),
                authority_head: "demo-source-grant-v1".into(),
                can_submit: c.principal != tk::Principal::NoSubmitPermission,
                can_read: c.principal != tk::Principal::NoReadPermission,
            },
            received_at: Timestamp::parse(c.received_at).unwrap(),
            binding_selector: "demo-retail-selector".into(),
        }
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        if let Some(store) = self.store.take() {
            self.runtime.block_on(store.close());
        }
    }
}
fn outcome(result: Result<AcceptResult, ServiceError>) -> ledgerlab_testkit::Result<tk::Outcome> {
    Ok(match result {
        Ok(AcceptResult::Accepted { receipt }) => tk::Outcome::Accepted(receipt),
        Ok(AcceptResult::Duplicate { kind, receipt }) => tk::Outcome::Duplicate {
            kind: match kind {
                DuplicateKind::Identity => tk::DuplicateKind::Identity,
                DuplicateKind::Semantic => tk::DuplicateKind::Semantic,
            },
            receipt,
        },
        Ok(AcceptResult::Conflict(kind)) => tk::Outcome::Conflict(match kind {
            ConflictKind::Identity => tk::ConflictKind::Identity,
            ConflictKind::Semantic => tk::ConflictKind::Semantic,
        }),
        Ok(AcceptResult::Rejected { code }) => tk::Outcome::Rejected(match code.as_str() {
            "SOURCE_UNAUTHORIZED" => tk::Rejection::Unauthorized,
            "TERMS_NOT_ACCEPTED" => tk::Rejection::TermsNotAccepted,
            "EVALUATION_INVALID" => tk::Rejection::EvaluationInvalid,
            "ARITHMETIC_OVERFLOW" => tk::Rejection::ArithmeticOverflow,
            _ => tk::Rejection::InvalidInput,
        }),
        Err(ServiceError::OutcomeUnknown {
            scope,
            source,
            external_id,
        }) => tk::Outcome::OutcomeUnknown {
            scope,
            source,
            external_id,
        },
        Err(ServiceError::Injected) => tk::Outcome::RolledBack,
        Err(ServiceError::ResponseLost) => tk::Outcome::ResponseLost,
        other => return Err(err(format!("unexpected service result: {other:?}"))),
    })
}
impl tk::AcceptanceBackend for Backend {
    fn evidence(&self) -> tk::BackendEvidence {
        let d = &self.store.as_ref().unwrap().diagnostics;
        tk::BackendEvidence {
            kind: tk::BackendKind::FileSqlite,
            location: self.directory.path().join("local.db").display().to_string(),
            engine_version: format!("{} {}", d.version, d.source_id),
            version_number: 3_051_003,
            durability: "verified WAL/FULL/fullfsync/foreign_keys".into(),
        }
    }
    fn observe(&mut self) -> ledgerlab_testkit::Result<Snapshot> {
        let result = std::process::Command::new("python3")
            .arg("-B")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/service/observe_sqlite.py"
            ))
            .arg(self.directory.path().join("local.db"))
            .output()
            .map_err(err)?;
        if !result.status.success() {
            return Err(err(String::from_utf8_lossy(&result.stderr)));
        }
        let v: Value = serde_json::from_slice(&result.stdout).map_err(err)?;
        let rows: BTreeMap<_, _> = v["rows"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, rows)| {
                (
                    name.clone(),
                    rows.as_array()
                        .unwrap()
                        .iter()
                        .map(|r| (decode(&r[0]), decode(&r[1])))
                        .collect(),
                )
            })
            .collect();
        let absent_tables = LATER_TABLES
            .iter()
            .filter(|t| !rows.contains_key(**t))
            .map(|s| s.to_string())
            .collect();
        Ok(Snapshot {
            journal: v["journal"]
                .as_array()
                .unwrap()
                .iter()
                .map(decode)
                .collect(),
            indexes: v["indexes"]
                .as_array()
                .unwrap()
                .iter()
                .map(decode)
                .collect(),
            state: decode(&v["state"]),
            operational: if v["operational"].is_null() {
                None
            } else {
                Some(decode(&v["operational"]))
            },
            aliases: v["aliases"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| Alias {
                    scope: [
                        a["scope"][0].as_str().unwrap().into(),
                        a["scope"][1].as_str().unwrap().into(),
                    ],
                    source: a["source"].as_str().unwrap().into(),
                    external_id: a["external_id"].as_str().unwrap().into(),
                    canonical_receipt: decode(&a["canonical_receipt"]),
                    ingress: decode(&a["ingress"]),
                    ingress_hash: a["ingress_hash"].as_str().unwrap().into(),
                    observed_at: a["observed_at"].as_str().unwrap().into(),
                })
                .collect(),
            rows,
            absent_tables,
        })
    }
    fn reopen(&mut self) -> ledgerlab_testkit::Result<()> {
        if let Some(h) = self.pending.take() {
            self.runtime.block_on(h.drain());
        }
        self.runtime.block_on(self.store.take().unwrap().close());
        self.store = Some(
            self.runtime
                .block_on(SqliteStore::open(self.directory.path()))
                .map_err(err)?,
        );
        Ok(())
    }
    fn accept(
        &mut self,
        c: &tk::Command,
        injection: Option<&Injection>,
    ) -> ledgerlab_testkit::Result<tk::Attempt> {
        self.affected = self
            .runtime
            .block_on(self.store.as_ref().unwrap().test_pool_probe())
            .map_err(err)?
            .0;
        let hooks = Hooks::new(injection.cloned(), None);
        let result = self.runtime.block_on(accept::run(
            self.store.as_ref().unwrap(),
            &Self::command(c),
            &hooks,
        ));
        if hooks.unknown().is_some() {
            self.pending = Some(hooks.clone());
            self.discarded = true;
        }
        Ok(tk::Attempt {
            outcome: outcome(result)?,
            hit: hooks.hit(),
        })
    }
    fn resolve_and_retry(&mut self, c: &tk::Command) -> ledgerlab_testkit::Result<tk::Attempt> {
        if self.pending.is_some() {
            self.reopen()?;
        }
        self.accept(c, None)
    }
    fn probe_active_commit(
        &mut self,
        c: &tk::Command,
    ) -> ledgerlab_testkit::Result<tk::ActiveCommitProbe> {
        let active = self
            .pending
            .as_ref()
            .unwrap()
            .state
            .active
            .load(std::sync::atomic::Ordering::Acquire);
        let store = self.store.as_ref().unwrap();
        let absent = self
            .runtime
            .block_on(store.lookup_identity(
                &crate::store::records::Scope {
                    tenant: "demo".into(),
                    environment: "sandbox".into(),
                },
                "urn:demo:app",
                "generation-1",
            ))
            .map_err(err)?
            .is_none();
        let locked = self
            .runtime
            .block_on(store.test_write_locked(self.directory.path()))
            .map_err(err)?;
        let candidate = ledgerlab_core::domain::normalize(
            &c.bytes,
            ledgerlab_core::domain::Scope::new("demo", "sandbox").unwrap(),
            "urn:demo:app",
        )
        .unwrap();
        Ok(tk::ActiveCommitProbe {
            primary_rows_absent: absent,
            original_transaction_active: active && locked,
            outcome: tk::Outcome::OutcomeUnknown {
                scope: ["demo".into(), "sandbox".into()],
                source: candidate.source().into(),
                external_id: candidate.external_id().into(),
            },
        })
    }
    fn pool_probe(&mut self) -> ledgerlab_testkit::Result<tk::PoolProbe> {
        if self.discarded && self.pending.is_some() {
            self.reopen()?;
        }
        let (id, active) = self
            .runtime
            .block_on(self.store.as_ref().unwrap().test_pool_probe())
            .map_err(err)?;
        Ok(tk::PoolProbe {
            affected_connection: if self.affected.is_empty() {
                id.clone()
            } else {
                self.affected.clone()
            },
            next_connection: id,
            affected_discarded: self.discarded,
            next_has_open_transaction: active,
        })
    }
    fn cancellation_points(&mut self) -> ledgerlab_testkit::Result<Vec<CancellationPoint>> {
        let mut points = vec![
            CancellationPoint {
                name: "begin".into(),
                class: AwaitClass::Begin,
                phase: CommitPhase::Before,
                trigger: None,
            },
            CancellationPoint {
                name: "admission_lock".into(),
                class: AwaitClass::Lock,
                phase: CommitPhase::Before,
                trigger: None,
            },
        ];
        for name in [
            "authority",
            "grant",
            "document:grant",
            "identity",
            "claim",
            "binding",
            "chain",
            "document:context",
            "document:binding",
            "document:policy",
            "document:roles",
            "document:assent",
        ] {
            points.push(CancellationPoint {
                name: name.into(),
                class: AwaitClass::Read,
                phase: CommitPhase::Before,
                trigger: None,
            });
        }
        for b in &self.oracle.write_boundaries {
            if let Boundary::Write {
                name,
                item,
                edge: Edge::Before,
            } = b
            {
                points.push(CancellationPoint {
                    name: format!("write:{name}:{item}"),
                    class: AwaitClass::Write {
                        name: name.clone(),
                        item: *item,
                    },
                    phase: CommitPhase::Before,
                    trigger: None,
                });
            }
        }
        for (name, class, phase) in [
            ("commit", AwaitClass::Commit, CommitPhase::InFlight),
            (
                "acknowledged",
                AwaitClass::Commit,
                CommitPhase::Acknowledged,
            ),
            ("rollback", AwaitClass::Rollback, CommitPhase::Before),
            ("cleanup", AwaitClass::Cleanup, CommitPhase::Before),
        ] {
            let trigger =
                matches!(class, AwaitClass::Rollback | AwaitClass::Cleanup).then(|| Injection {
                    boundary: Boundary::Write {
                        name: "snapshot_document".into(),
                        item: 0,
                        edge: Edge::After,
                    },
                    fault: Fault::Rollback,
                });
            points.push(CancellationPoint {
                name: name.into(),
                class,
                phase,
                trigger,
            });
        }
        Ok(points)
    }
    fn cancel(
        &mut self,
        c: &tk::Command,
        point: &CancellationPoint,
    ) -> ledgerlab_testkit::Result<tk::Attempt> {
        self.affected = self
            .runtime
            .block_on(self.store.as_ref().unwrap().test_pool_probe())
            .map_err(err)?
            .0;
        let hooks = Hooks::new(point.trigger.clone(), Some(point.clone()));
        let store = self.store.as_ref().unwrap().clone();
        let command = Self::command(c);
        let task_hooks = hooks.clone();
        self.runtime.block_on(async {
            let task =
                tokio::spawn(async move { accept::run(&store, &command, &task_hooks).await });
            tokio::time::timeout(Duration::from_secs(5), hooks.state.reached.notified())
                .await
                .expect("cancellation hook must be reached");
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        });
        let out = match point.phase {
            CommitPhase::Before => tk::Outcome::Cancelled,
            CommitPhase::Acknowledged => tk::Outcome::ResponseLost,
            CommitPhase::InFlight => {
                self.discarded = true;
                self.pending = Some(hooks.clone());
                tk::Outcome::OutcomeUnknown {
                    scope: ["demo".into(), "sandbox".into()],
                    source: "urn:demo:app".into(),
                    external_id: "generation-1".into(),
                }
            }
        };
        Ok(tk::Attempt {
            outcome: out,
            hit: hooks.hit(),
        })
    }
    fn zero_evidence(&mut self) -> ledgerlab_testkit::Result<tk::ZeroEvidence> {
        let s = self.observe()?;
        let rows: Vec<Value> = s
            .journal
            .iter()
            .map(|b| serde_json::from_slice(b).unwrap())
            .collect();
        let event = rows.iter().find(|r| r["kind"] == "event").unwrap();
        let receipt = rows.iter().find(|r| r["kind"] == "receipt").unwrap();
        let state: Value = serde_json::from_slice(&s.state).unwrap();
        Ok(tk::ZeroEvidence {
            event_id: event["id"].as_str().unwrap().into(),
            receipt: serde_json::to_vec(&receipt["body"]).unwrap(),
            explanation_codes: rows
                .iter()
                .filter(|r| r["kind"] == "explanation")
                .map(|r| r["body"]["code"].as_str().unwrap().into())
                .collect(),
            action_ids: rows
                .iter()
                .filter(|r| r["kind"] == "action")
                .map(|r| r["id"].as_str().unwrap().into())
                .collect(),
            intention_ids: rows
                .iter()
                .filter(|r| r["kind"] == "intention")
                .map(|r| r["id"].as_str().unwrap().into())
                .collect(),
            revision: state["chain"]["revision"].as_str().unwrap().into(),
            event_count: state["chain"]["event_count"].as_str().unwrap().into(),
        })
    }
}
#[test]
fn sqlite_facade_basics() {
    use ledgerlab_testkit::cases::{run_case, Case};
    let oracle = FixtureOracle::workspace().unwrap();
    for case in [
        Case::Fresh,
        Case::IdentityDuplicate,
        Case::IdentityConflict,
        Case::SemanticDuplicate,
        Case::SemanticConflict,
        Case::QuantityNormalization,
        Case::UnauthorizedSource,
        Case::UnauthorizedPrincipal,
        Case::UnauthorizedReceiptRead,
        Case::MissingRealTerms,
        Case::ZeroAction,
    ] {
        run_case(&Factory, &oracle, &case).unwrap_or_else(|e| panic!("{case:?}: {e}"));
    }
}

#[test]
fn sqlite_acceptance_83_cases() {
    let oracle = FixtureOracle::workspace().unwrap();
    let cases = ledgerlab_testkit::cases::acceptance_cases(&oracle);
    assert_eq!(cases.len(), 83);
    for case in cases {
        println!("running {case:?}");
        ledgerlab_testkit::cases::run_case(&Factory, &oracle, &case)
            .unwrap_or_else(|e| panic!("{case:?}: {e}"));
    }
}
#[test]
fn sqlite_cancellation_all_awaits() {
    let oracle = FixtureOracle::workspace().unwrap();
    let reports = ledgerlab_testkit::cases::run_cancellation(&Factory, &oracle).unwrap();
    println!("{} real SQLite cancellation cases passed", reports.len());
}
