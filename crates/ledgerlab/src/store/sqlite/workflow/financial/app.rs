//! Installation-wide app attribution and once-only evidence consumption.
//! These are mechanical complete-candidate effects, never source authentication.
use super::effects::{revision, same_text};
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::{
    EconomicCandidate, ExactRef, PermanentClaimKey,
};
impl SqliteTx {
    pub(super) async fn insert_app_claim(
        &mut self,
        candidate: &EconomicCandidate,
        claim: &PermanentClaimKey,
        r: &ExactRef,
        session: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(self.conn())
            .await?;
        if !resource_compare(session, version, 15)?.is_eq() {
            return Err(StoreError::BillingUpgradeRequired.into());
        }
        let (store, expected_kind, sql, parts): (&str, &str, &'static str, [&str; 5]) = match claim
        {
            PermanentClaimKey::AppCall {
                logical_store_id,
                source_namespace,
                call_id,
            } => (
                logical_store_id,
                "workflow-fact-attestation",
                "INSERT INTO workflow_app_call_claims VALUES(?,?,?,?,?,?)",
                [source_namespace, call_id, "", "", ""],
            ),
            PermanentClaimKey::AppProviderCall {
                logical_store_id,
                provider,
                account,
                request_id,
            } => (
                logical_store_id,
                "workflow-fact-attestation",
                "INSERT INTO workflow_app_provider_calls VALUES(?,?,?,?,?,?,?)",
                [provider, account, request_id, "", ""],
            ),
            PermanentClaimKey::AppComponentObservation {
                logical_store_id,
                original_work_ref,
                component_slot,
            } => {
                if !same_text(&original_work_ref.kind, "workflow-work", session)? {
                    return Err(StoreError::Integrity("app observation original work kind").into());
                }
                (
                    logical_store_id,
                    "workflow-fact-attestation",
                    "INSERT INTO workflow_app_component_observations VALUES(?,?,?,?,?,?,?,?)",
                    [
                        &original_work_ref.kind,
                        &original_work_ref.id,
                        &original_work_ref.digest,
                        component_slot,
                        "",
                    ],
                )
            }
            PermanentClaimKey::AppCostComponent {
                logical_store_id,
                original_work_ref,
                component_slot,
            } => {
                if !same_text(&original_work_ref.kind, "workflow-work", session)? {
                    return Err(StoreError::Integrity("app claim original work kind").into());
                }
                (
                    logical_store_id,
                    "workflow-cost",
                    "INSERT INTO workflow_app_cost_claims VALUES(?,?,?,?,?,?,?,?)",
                    [
                        &original_work_ref.kind,
                        &original_work_ref.id,
                        &original_work_ref.digest,
                        component_slot,
                        "",
                    ],
                )
            }
            PermanentClaimKey::AppFinalEvidence {
                logical_store_id,
                final_evidence_ref,
            } => {
                if !same_text(
                    &final_evidence_ref.kind,
                    "workflow-fact-attestation",
                    session,
                )? {
                    return Err(StoreError::Integrity("app final evidence kind").into());
                }
                (
                    logical_store_id,
                    "workflow-cost",
                    "INSERT INTO workflow_app_evidence_consumptions VALUES(?,?,?,?,?,?,?)",
                    [
                        &final_evidence_ref.kind,
                        &final_evidence_ref.id,
                        &final_evidence_ref.digest,
                        "",
                        "",
                    ],
                )
            }
            PermanentClaimKey::AppObservationRevision {
                logical_store_id,
                observation_ref,
                cost_revision,
            } => {
                if !same_text(&observation_ref.kind, "workflow-fact-attestation", session)? {
                    return Err(StoreError::Integrity("app observation kind").into());
                }
                revision(cost_revision, session)?;
                (
                    logical_store_id,
                    "workflow-cost",
                    "INSERT INTO workflow_app_observation_revisions VALUES(?,?,?,?,?,?,?,?)",
                    [
                        &observation_ref.kind,
                        &observation_ref.id,
                        &observation_ref.digest,
                        cost_revision,
                        "",
                    ],
                )
            }
            _ => {
                return Err(StoreError::Integrity("non-app claim routed to app projection").into());
            }
        };
        if !same_text(store, &candidate.key().logical_store_id, session)?
            || !same_text(&r.kind, expected_kind, session)?
        {
            return Err(StoreError::Integrity("app claim store/kind").into());
        }
        // Every key is mandatory and nonempty. Fixed arity is selected by the
        // actual typed variant, never by skipping an attacker-controlled value.
        let arity = match claim {
            PermanentClaimKey::AppCall { .. } => 2,
            PermanentClaimKey::AppProviderCall { .. }
            | PermanentClaimKey::AppFinalEvidence { .. } => 3,
            _ => 4,
        };
        for value in parts.iter().take(arity) {
            session.semantic_work().visit(ValidationVisit::Constraint)?;
            session.semantic_work().arithmetic(
                ArithmeticOperation::Compare,
                &[&value.len().into(), &0usize.into()],
            )?;
            if value.is_empty() {
                return Err(StoreError::Integrity("empty app permanent key").into());
            }
        }
        let values = [
            store, parts[0], parts[1], parts[2], parts[3], &r.kind, &r.id, &r.digest,
        ];
        // Select a fixed borrowed parameter window; no unleased key Vec/JSON.
        let (values, count) = match arity {
            2 => (
                [store, parts[0], parts[1], &r.kind, &r.id, &r.digest, "", ""],
                6,
            ),
            3 => (
                [
                    store, parts[0], parts[1], parts[2], &r.kind, &r.id, &r.digest, "",
                ],
                7,
            ),
            _ => (values, 8),
        };
        let bytes = values.map(str::as_bytes);
        let parameters = &bytes[..count];
        let payload = io::binding_payload_sets(
            parameters,
            &[parameters, parameters, parameters],
            3,
            0,
            session,
        )?;
        self.financial_reserve_io(session, payload)?;
        let mut query = sqlx::query(sql);
        for value in values.iter().take(count) {
            session.semantic_work().visit(ValidationVisit::Element)?;
            query = query.bind(*value);
        }
        query.execute(self.conn()).await?;
        self.financial_finish_io(session).await
    }
}

/// An exact retained projection hit, still untrusted until original semantic
/// replay and immutable-body/currentness checks finish under the held writer.
pub(crate) struct AppClaimLocator {
    pub kind: AuditedBytes,
    pub id: AuditedBytes,
    pub digest: AuditedBytes,
}
struct Lookup<'a> {
    store: &'a str,
    lengths: &'static str,
    values: &'static str,
    parts: [&'a str; 4],
    arity: usize,
}
fn lookup<'a>(key: &'a PermanentClaimKey) -> Result<Lookup<'a>, FinancialReadError> {
    let (store, table, parts, arity) = match key {
        PermanentClaimKey::AppCall {
            logical_store_id,
            source_namespace,
            call_id,
        } => (
            logical_store_id.as_str(),
            "call",
            [source_namespace.as_str(), call_id.as_str(), "", ""],
            2,
        ),
        PermanentClaimKey::AppProviderCall {
            logical_store_id,
            provider,
            account,
            request_id,
        } => (
            logical_store_id.as_str(),
            "provider",
            [provider.as_str(), account.as_str(), request_id.as_str(), ""],
            3,
        ),
        PermanentClaimKey::AppComponentObservation {
            logical_store_id,
            original_work_ref,
            component_slot,
        } => (
            logical_store_id.as_str(),
            "observation",
            [
                original_work_ref.kind.as_str(),
                original_work_ref.id.as_str(),
                original_work_ref.digest.as_str(),
                component_slot.as_str(),
            ],
            4,
        ),
        PermanentClaimKey::AppCostComponent {
            logical_store_id,
            original_work_ref,
            component_slot,
        } => (
            logical_store_id.as_str(),
            "claim",
            [
                original_work_ref.kind.as_str(),
                original_work_ref.id.as_str(),
                original_work_ref.digest.as_str(),
                component_slot.as_str(),
            ],
            4,
        ),
        PermanentClaimKey::AppFinalEvidence {
            logical_store_id,
            final_evidence_ref,
        } => (
            logical_store_id.as_str(),
            "evidence",
            [
                final_evidence_ref.kind.as_str(),
                final_evidence_ref.id.as_str(),
                final_evidence_ref.digest.as_str(),
                "",
            ],
            3,
        ),
        PermanentClaimKey::AppObservationRevision {
            logical_store_id,
            observation_ref,
            cost_revision,
        } => (
            logical_store_id.as_str(),
            "revision",
            [
                observation_ref.kind.as_str(),
                observation_ref.id.as_str(),
                observation_ref.digest.as_str(),
                cost_revision.as_str(),
            ],
            4,
        ),
        _ => return Err(StoreError::Integrity("non-app projection lookup").into()),
    };
    // SQL literals preserve independent installation-wide keys, no scoped
    // fallback and no ID-only latest selection for an original full Ref.
    let (lengths, values) = match table {
        "call" => (
            "SELECT octet_length(record_kind),octet_length(record_id),octet_length(record_digest) FROM workflow_app_call_claims WHERE logical_store_id=? AND source_namespace=? AND call_id=?",
            "SELECT record_kind,record_id,record_digest FROM workflow_app_call_claims WHERE logical_store_id=? AND source_namespace=? AND call_id=?",
        ),
        "provider" => (
            "SELECT octet_length(record_kind),octet_length(record_id),octet_length(record_digest) FROM workflow_app_provider_calls WHERE logical_store_id=? AND provider=? AND account=? AND request_id=?",
            "SELECT record_kind,record_id,record_digest FROM workflow_app_provider_calls WHERE logical_store_id=? AND provider=? AND account=? AND request_id=?",
        ),
        "observation" => (
            "SELECT octet_length(record_kind),octet_length(record_id),octet_length(record_digest) FROM workflow_app_component_observations WHERE logical_store_id=? AND work_kind=? AND work_id=? AND work_digest=? AND component_slot=?",
            "SELECT record_kind,record_id,record_digest FROM workflow_app_component_observations WHERE logical_store_id=? AND work_kind=? AND work_id=? AND work_digest=? AND component_slot=?",
        ),
        "claim" => (
            "SELECT octet_length(record_kind),octet_length(record_id),octet_length(record_digest) FROM workflow_app_cost_claims WHERE logical_store_id=? AND work_kind=? AND work_id=? AND work_digest=? AND component_slot=?",
            "SELECT record_kind,record_id,record_digest FROM workflow_app_cost_claims WHERE logical_store_id=? AND work_kind=? AND work_id=? AND work_digest=? AND component_slot=?",
        ),
        "evidence" => (
            "SELECT octet_length(record_kind),octet_length(record_id),octet_length(record_digest) FROM workflow_app_evidence_consumptions WHERE logical_store_id=? AND evidence_kind=? AND evidence_id=? AND evidence_digest=?",
            "SELECT record_kind,record_id,record_digest FROM workflow_app_evidence_consumptions WHERE logical_store_id=? AND evidence_kind=? AND evidence_id=? AND evidence_digest=?",
        ),
        _ => (
            "SELECT octet_length(record_kind),octet_length(record_id),octet_length(record_digest) FROM workflow_app_observation_revisions WHERE logical_store_id=? AND observation_kind=? AND observation_id=? AND observation_digest=? AND cost_revision=?",
            "SELECT record_kind,record_id,record_digest FROM workflow_app_observation_revisions WHERE logical_store_id=? AND observation_kind=? AND observation_id=? AND observation_digest=? AND cost_revision=?",
        ),
    };
    Ok(Lookup {
        store,
        lengths,
        values,
        parts,
        arity,
    })
}
fn bind_lookup<'q>(
    sql: &'static str,
    values: &[&'q str],
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments> {
    let mut query = sqlx::query(sql);
    for value in values {
        query = query.bind(*value);
    }
    query
}
impl SqliteTx {
    pub(crate) async fn workflow_app_claim(
        &mut self,
        key: &PermanentClaimKey,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<AppClaimLocator>, FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned app observation").into());
        }
        self.failed = true;
        let result = timeout_at(self.deadline, self.app_claim_inner(key, session))
            .await
            .map_err(|_| FinancialReadError::Store(StoreError::Deadline))?;
        if result.is_ok() {
            session.semantic_work().visit(ValidationVisit::Constraint)?;
            session.check_cancelled()?;
            if self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
                || self.financial_io.is_some()
            {
                return Err(StoreError::Integrity("app observation retained failed worker").into());
            }
            self.failed = false;
        }
        result
    }
    async fn app_claim_inner(
        &mut self,
        key: &PermanentClaimKey,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<AppClaimLocator>, FinancialReadError> {
        session.semantic_work().visit(ValidationVisit::RefLookup)?;
        let selected = lookup(key)?;
        session.check_cancelled()?;
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(self.conn())
            .await?;
        if !resource_compare(session, version, 15)?.is_eq() {
            return Err(StoreError::BillingUpgradeRequired.into());
        }
        let values = [
            selected.store,
            selected.parts[0],
            selected.parts[1],
            selected.parts[2],
            selected.parts[3],
        ];
        session.semantic_work().arithmetic(
            ArithmeticOperation::Add,
            &[&selected.arity.into(), &1usize.into()],
        )?;
        let count = selected.arity + 1;
        let parameters = &values[..count];
        let bytes = values.map(str::as_bytes);
        let bound = &bytes[..count];
        let payload = io::binding_payload(bound, &[], 0, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        let lengths = bind_lookup(selected.lengths, parameters)
            .fetch_optional(self.conn())
            .await?;
        let Some(lengths) = lengths else {
            self.financial_finish_io(session).await?;
            return Ok(None);
        };
        for (position, maximum) in [(0usize, 48i64), (1, 104), (2, 71)] {
            session.semantic_work().visit(ValidationVisit::Field)?;
            let n: i64 = lengths.try_get(position)?;
            session.semantic_work().arithmetic(
                ArithmeticOperation::Multiply,
                &[&maximum.into(), &2i64.into()],
            )?;
            if !resource_compare(session, n, 0)?.is_gt()
                || resource_compare(session, n, 2 * maximum)?.is_gt()
            {
                return Err(StoreError::Integrity("app projection byte preflight").into());
            }
        }
        drop(lengths);
        self.financial_finish_io(session).await?;
        // Same pinned direct-byte probe/UTF16 conversion bound as head reads.
        // Reserve conversion/result passes before fetching any full TEXT value.
        session.semantic_work().scan_bytes(8 * (48 + 104 + 71))?;
        session.semantic_work().write_bytes(8 * (48 + 104 + 71))?;
        let payload = io::binding_payload(bound, &[], 0, 8 * (48 + 104 + 71), session)?;
        self.financial_reserve_io(session, payload)?;
        let row = bind_lookup(selected.values, parameters)
            .fetch_optional(self.conn())
            .await?
            .ok_or_else(|| StoreError::Integrity("app projection vanished in held snapshot"))?;
        for (position, maximum) in [(0usize, 48i64), (1, 104), (2, 71)] {
            session.semantic_work().visit(ValidationVisit::Field)?;
            let text: &str = row.try_get(position)?;
            if !resource_compare(session, text.len() as i64, 0)?.is_gt()
                || resource_compare(session, text.len() as i64, maximum)?.is_gt()
            {
                return Err(StoreError::Integrity("app projection UTF8 bound").into());
            }
        }
        let kind = session.copy_representation(
            row.try_get::<&str, _>(0)?.as_bytes(),
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        let id = session.copy_representation(
            row.try_get::<&str, _>(1)?.as_bytes(),
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        let digest = session.copy_representation(
            row.try_get::<&str, _>(2)?.as_bytes(),
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        drop(row);
        self.financial_finish_io(session).await?;
        Ok(Some(AppClaimLocator { kind, id, digest }))
    }
}
