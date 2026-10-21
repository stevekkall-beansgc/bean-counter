//! Mechanical agreement ownership, independent heads and immutable candidates.
//! Indexed rows never certify assent, grants, schedule selection or origin.
use super::{JournalAppend, RecordRef, SqliteTx, StoreError};
use sqlx::Row;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HeadKind {
    Acceptance,
    Activation,
}

impl HeadKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Acceptance => "agreement_acceptance",
            Self::Activation => "agreement_activation",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgreementHead {
    pub revision: i64,
    pub transition: RecordRef,
}
#[derive(Clone, Debug)]
pub(crate) struct AcceptanceProjection {
    pub customer: String,
    pub agreement: String,
    pub expected_revision: i64,
    pub head: AgreementHead,
    pub accepted: RecordRef,
    pub binding: RecordRef,
    pub intent: RecordRef,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActivationAction {
    Activate,
    Replace,
    Revoke,
}
impl ActivationAction {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Activate => "activate",
            Self::Replace => "replace",
            Self::Revoke => "revoke",
        }
    }
    fn parse(s: &str) -> Result<Self, StoreError> {
        match s {
            "activate" => Ok(Self::Activate),
            "replace" => Ok(Self::Replace),
            "revoke" => Ok(Self::Revoke),
            _ => Err(StoreError::Integrity("invalid activation index action")),
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct ActivationProjection {
    pub customer: String,
    pub agreement: String,
    pub expected_revision: i64,
    pub head: AgreementHead,
    pub activation: RecordRef,
    pub binding: RecordRef,
    pub intent: RecordRef,
    pub action: ActivationAction,
    pub effective_at_us: i64,
    pub accepted_at_us: i64,
    pub sequence: i64,
    pub target: Option<RecordRef>,
}
#[derive(Clone, Debug)]
pub(crate) struct ActivationCandidate {
    pub head: AgreementHead,
    pub activation: RecordRef,
    pub binding: RecordRef,
    pub intent: RecordRef,
    pub action: ActivationAction,
    pub effective_at_us: i64,
    pub accepted_at_us: i64,
    pub sequence: i64,
    pub target: Option<RecordRef>,
}
fn row_ref(row: &sqlx::sqlite::SqliteRow, prefix: &str) -> Result<RecordRef, StoreError> {
    // Prefixes are closed query aliases below, never interpolated into SQL.
    let required = |field: &str| -> Result<String, StoreError> {
        row.try_get::<Option<String>, _>(format!("{prefix}_{field}").as_str())?
            .ok_or(StoreError::Integrity(
                "dangling agreement projection reference",
            ))
    };
    let reference = RecordRef {
        kind: required("kind")?,
        id: required("id")?,
        digest: required("digest")?,
    };
    let expected = match prefix {
        "head" => "workflow-head-transition",
        "accepted" => "workflow-accepted-agreement",
        "activation" | "target" => "workflow-activation",
        "binding" => "workflow-binding",
        "intent" => "workflow-agreement-intent",
        _ => {
            return Err(StoreError::Integrity(
                "unknown agreement projection reference",
            ));
        }
    };
    if reference.kind != expected {
        return Err(StoreError::Integrity(
            "wrong agreement projection reference family",
        ));
    }
    Ok(reference)
}
fn projection_document(p: &JournalAppend, r: &RecordRef) -> Result<(), StoreError> {
    if !p.documents.iter().any(|d| d.reference == *r) {
        return Err(StoreError::Integrity(
            "agreement projection outside complete prepared closure",
        ));
    }
    Ok(())
}
fn projection_body(p: &JournalAppend, r: &RecordRef) -> Result<serde_json::Value, StoreError> {
    let d = p
        .documents
        .iter()
        .find(|d| d.reference == *r)
        .ok_or(StoreError::Integrity("projection document absent"))?;
    let body = ledgerlab_core::canonical::parse_bounded(&d.bytes, 262144)
        .map_err(|_| StoreError::Integrity("invalid projection body"))?;
    if ledgerlab_core::canonical::CanonicalBytes::from_value(&body)
        .map_err(|_| StoreError::Integrity("invalid projection JCS"))?
        .as_slice()
        != d.bytes
        || body["schema"] != d.schema
    {
        return Err(StoreError::Integrity(
            "projection body/schema copies differ",
        ));
    }
    Ok(body)
}
fn activation_copies(p: &JournalAppend, a: &ActivationProjection) -> Result<(), StoreError> {
    use serde_json::json;
    let head = projection_body(p, &a.head.transition)?;
    let activation = projection_body(p, &a.activation)?;
    let key = json!({"logical_store_id":p.store,"scope":[p.tenant,p.environment],"source":p.source,"operation_id":p.operation_id});
    let old = a.expected_revision.to_string();
    let new = a.head.revision.to_string();
    let sequence = a.sequence.to_string();
    let micros = |v: &serde_json::Value| -> Result<i64, StoreError> {
        let s = v
            .as_str()
            .ok_or(StoreError::Integrity("projection time string"))?;
        ledgerlab_core::domain::Timestamp::parse(s)
            .map(|t| t.micros())
            .map_err(|_| StoreError::Integrity("projection time"))
    };
    if head["schema"] != "ledger-workflow-head-transition/1"
        || head["head_kind"] != "agreement_activation"
        || head["operation_key"] != key
        || head["scope"] != key["scope"]
        || head["customer"] != a.customer
        || head["agreement_id"] != a.agreement
        || head["old_revision"] != old.as_str()
        || head["new_revision"] != new.as_str()
        || head["accepted_binding_ref"] != json!(a.binding)
        || head["commit_sequence"] != sequence.as_str()
        || micros(&head["accepted_at"])? != a.accepted_at_us
        || micros(&head["resolved_effective_time"])? != a.effective_at_us
        || (a.expected_revision == 0) != head.get("prior_head_ref").is_none()
        || activation["schema"] != "ledger-workflow-activation-transition/1"
        || activation["head_transition_ref"] != json!(a.head.transition)
        || activation["binding_ref"] != json!(a.binding)
        || activation["action"] != a.action.as_str()
        || activation["commit_sequence"] != sequence.as_str()
        || micros(&activation["accepted_at"])? != a.accepted_at_us
        || micros(&activation["effective_at"])? != a.effective_at_us
        || activation.get("target_transition_ref") != a.target.as_ref().map(|r| json!(r)).as_ref()
        || a.effective_at_us < a.accepted_at_us
        || (a.action == ActivationAction::Revoke && a.effective_at_us != a.accepted_at_us)
    {
        return Err(StoreError::Integrity(
            "activation projection body copies differ",
        ));
    }
    Ok(())
}
impl SqliteTx {
    pub(crate) async fn workflow_agreement_owner(
        &mut self,
        store: &str,
        scope: &[String; 2],
        agreement: &str,
    ) -> Result<Option<String>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result=crate::store::sqlite::workflow::until(self.workflow_deadline(),async {self.workflow_schema().await?;sqlx::query_scalar("SELECT customer FROM workflow_agreement_owners WHERE logical_store_id=? AND tenant=? AND environment=? AND agreement_id=?").bind(store).bind(&scope[0]).bind(&scope[1]).bind(agreement).fetch_optional(self.conn()).await.map_err(StoreError::from)}).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
    pub(crate) async fn workflow_agreement_head(
        &mut self,
        store: &str,
        scope: &[String; 2],
        customer: &str,
        agreement: &str,
        kind: HeadKind,
    ) -> Result<Option<AgreementHead>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result=crate::store::sqlite::workflow::until(self.workflow_deadline(),async {self.workflow_schema().await?;let row=sqlx::query("SELECT h.revision,d.kind AS head_kind,d.id AS head_id,d.digest AS head_digest FROM workflow_agreement_heads h LEFT JOIN workflow_documents d ON d.id=h.transition_id WHERE h.logical_store_id=? AND h.tenant=? AND h.environment=? AND h.customer=? AND h.agreement_id=? AND h.head_kind=?").bind(store).bind(&scope[0]).bind(&scope[1]).bind(customer).bind(agreement).bind(kind.as_str()).fetch_optional(self.conn()).await?;row.map(|r|Ok::<_,StoreError>(AgreementHead{revision:r.try_get("revision")?,transition:row_ref(&r,"head")?})).transpose()}).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
    pub(crate) async fn workflow_agreement_accepted_binding(
        &mut self,
        store: &str,
        scope: &[String; 2],
        binding: &RecordRef,
    ) -> Result<Option<RecordRef>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result=crate::store::sqlite::workflow::until(self.workflow_deadline(),async {self.workflow_schema().await?;let row=sqlx::query("SELECT d.kind AS accepted_kind,d.id AS accepted_id,d.digest AS accepted_digest FROM workflow_agreement_acceptances a JOIN workflow_documents b ON b.id=a.binding_id JOIN workflow_documents d ON d.id=a.accepted_id WHERE a.logical_store_id=? AND a.tenant=? AND a.environment=? AND b.kind=? AND b.id=? AND b.digest=?").bind(store).bind(&scope[0]).bind(&scope[1]).bind(&binding.kind).bind(&binding.id).bind(&binding.digest).fetch_optional(self.conn()).await?;row.map(|r|row_ref(&r,"accepted")).transpose()}).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
    /// Locate immutable activation births independently of the mutable head and
    /// activation index. Surviving head identities remain candidates even when
    /// their operation row is missing; the host must refuse that missing birth.
    pub(crate) async fn workflow_agreement_activation_prefix(
        &mut self,
        store: &str,
        scope: &[String; 2],
        customer: &str,
        agreement: &str,
    ) -> Result<Vec<Vec<u8>>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result=crate::store::sqlite::workflow::until(self.workflow_deadline(),async {
            self.workflow_schema().await?;
            sqlx::query_scalar("SELECT identity_key FROM workflow_operations WHERE json_extract(operation_bytes,'$.kind') IN ('agreement.activate','agreement.revoke') AND json_extract(operation_bytes,'$.key.logical_store_id')=? AND json_extract(operation_bytes,'$.key.scope[0]')=? AND json_extract(operation_bytes,'$.key.scope[1]')=? AND json_extract(operation_bytes,'$.request_evidence.normalized_request.payload.agreement_id')=? UNION SELECT CAST(json_extract(canonical_bytes,'$.operation_key') AS BLOB) FROM workflow_documents WHERE json_extract(canonical_bytes,'$.schema')='ledger-workflow-head-transition/1' AND json_extract(canonical_bytes,'$.head_kind')='agreement_activation' AND json_extract(canonical_bytes,'$.operation_key.logical_store_id')=? AND json_extract(canonical_bytes,'$.operation_key.scope[0]')=? AND json_extract(canonical_bytes,'$.operation_key.scope[1]')=? AND json_extract(canonical_bytes,'$.customer')=? AND json_extract(canonical_bytes,'$.agreement_id')=?")
                .bind(store).bind(&scope[0]).bind(&scope[1]).bind(agreement)
                .bind(store).bind(&scope[0]).bind(&scope[1]).bind(customer).bind(agreement)
                .fetch_all(self.conn()).await.map_err(StoreError::from)
        }).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
    pub(crate) async fn workflow_agreement_activation_history(
        &mut self,
        store: &str,
        scope: &[String; 2],
        customer: &str,
        agreement: &str,
    ) -> Result<Vec<ActivationCandidate>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result=crate::store::sqlite::workflow::until(self.workflow_deadline(),async {
            self.workflow_schema().await?;
            let rows=sqlx::query("SELECT a.revision,a.action,a.effective_at_us,a.accepted_at_us,a.commit_sequence,a.target_id AS expected_target_id,h.kind AS head_kind,h.id AS head_id,h.digest AS head_digest,d.kind AS activation_kind,d.id AS activation_id,d.digest AS activation_digest,b.kind AS binding_kind,b.id AS binding_id,b.digest AS binding_digest,i.kind AS intent_kind,i.id AS intent_id,i.digest AS intent_digest,t.kind AS target_kind,t.id AS target_id,t.digest AS target_digest FROM workflow_agreement_activations a LEFT JOIN workflow_documents h ON h.id=a.transition_id LEFT JOIN workflow_documents d ON d.id=a.activation_id LEFT JOIN workflow_documents b ON b.id=a.binding_id LEFT JOIN workflow_documents i ON i.id=a.intent_id LEFT JOIN workflow_documents t ON t.id=a.target_id WHERE a.logical_store_id=? AND a.tenant=? AND a.environment=? AND a.customer=? AND a.agreement_id=? ORDER BY a.revision").bind(store).bind(&scope[0]).bind(&scope[1]).bind(customer).bind(agreement).fetch_all(self.conn()).await?;
            rows.into_iter().map(|r|Ok(ActivationCandidate{head:AgreementHead{revision:r.try_get("revision")?,transition:row_ref(&r,"head")?},activation:row_ref(&r,"activation")?,binding:row_ref(&r,"binding")?,intent:row_ref(&r,"intent")?,action:ActivationAction::parse(r.try_get("action")?)?,effective_at_us:r.try_get("effective_at_us")?,accepted_at_us:r.try_get("accepted_at_us")?,sequence:r.try_get("commit_sequence")?,target:if r.try_get::<Option<String>,_>("expected_target_id")?.is_some(){Some(row_ref(&r,"target")?)}else{None}})).collect::<Result<Vec<_>,StoreError>>()
        }).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
    async fn workflow_agreement_advance_head(
        &mut self,
        p: &JournalAppend,
        customer: &str,
        agreement: &str,
        kind: HeadKind,
        expected: i64,
        head: &AgreementHead,
    ) -> Result<(), StoreError> {
        projection_document(p, &head.transition)?;
        if expected < 0
            || expected.checked_add(1) != Some(head.revision)
            || head.transition.kind != "workflow-head-transition"
        {
            return Err(StoreError::Integrity("invalid agreement head increment"));
        }
        let owner = self
            .workflow_agreement_owner(
                &p.store,
                &[p.tenant.clone(), p.environment.clone()],
                agreement,
            )
            .await?;
        if owner.as_ref().is_some_and(|old| old != customer) {
            return Err(StoreError::DeliveryConflict);
        }
        if owner.is_none() {
            if kind != HeadKind::Acceptance || expected != 0 {
                return Err(StoreError::ExpectedCurrent);
            }
            sqlx::query("INSERT INTO workflow_agreement_owners VALUES(?,?,?,?,?)")
                .bind(&p.store)
                .bind(&p.tenant)
                .bind(&p.environment)
                .bind(agreement)
                .bind(customer)
                .execute(self.conn())
                .await?;
        }
        if expected == 0 {
            sqlx::query("INSERT INTO workflow_agreement_heads VALUES(?,?,?,?,?,?,?,?)")
                .bind(&p.store)
                .bind(&p.tenant)
                .bind(&p.environment)
                .bind(agreement)
                .bind(customer)
                .bind(kind.as_str())
                .bind(head.revision)
                .bind(&head.transition.id)
                .execute(self.conn())
                .await?;
        } else {
            let changed=sqlx::query("UPDATE workflow_agreement_heads SET revision=?,transition_id=? WHERE logical_store_id=? AND tenant=? AND environment=? AND agreement_id=? AND customer=? AND head_kind=? AND revision=?").bind(head.revision).bind(&head.transition.id).bind(&p.store).bind(&p.tenant).bind(&p.environment).bind(agreement).bind(customer).bind(kind.as_str()).bind(expected).execute(self.conn()).await?;
            if changed.rows_affected() != 1 {
                return Err(StoreError::ExpectedCurrent);
            }
        }
        Ok(())
    }
    pub(super) async fn workflow_agreement_insert_acceptance(
        &mut self,
        p: &JournalAppend,
        a: &AcceptanceProjection,
    ) -> Result<(), StoreError> {
        for r in [&a.accepted, &a.binding, &a.intent] {
            projection_document(p, r)?;
        }
        if p.operation.kind != "agreement.accept"
            || a.accepted.kind != "workflow-accepted-agreement"
            || a.binding.kind != "workflow-binding"
            || a.intent.kind != "workflow-agreement-intent"
        {
            return Err(StoreError::Integrity(
                "invalid acceptance projection families",
            ));
        }
        self.workflow_agreement_advance_head(
            p,
            &a.customer,
            &a.agreement,
            HeadKind::Acceptance,
            a.expected_revision,
            &a.head,
        )
        .await?;
        sqlx::query("INSERT INTO workflow_agreement_acceptances VALUES(?,?,?,?,?,?,?,?,?,?)")
            .bind(&p.store)
            .bind(&p.tenant)
            .bind(&p.environment)
            .bind(&a.agreement)
            .bind(&a.customer)
            .bind(a.head.revision)
            .bind(&a.accepted.id)
            .bind(&a.binding.id)
            .bind(&a.intent.id)
            .bind(&a.head.transition.id)
            .execute(self.conn())
            .await?;
        Ok(())
    }
    pub(super) async fn workflow_agreement_insert_activation(
        &mut self,
        p: &JournalAppend,
        a: &ActivationProjection,
    ) -> Result<(), StoreError> {
        activation_copies(p, a)?;
        for r in [&a.activation, &a.binding, &a.intent] {
            projection_document(p, r)?;
        }
        if let Some(target) = &a.target {
            projection_document(p, target)?;
        }
        let kind = if a.action == ActivationAction::Revoke {
            "agreement.revoke"
        } else {
            "agreement.activate"
        };
        if p.operation.kind != kind
            || a.activation.kind != "workflow-activation"
            || a.binding.kind != "workflow-binding"
            || a.intent.kind != "workflow-agreement-intent"
            || a.target
                .as_ref()
                .is_some_and(|t| t.kind != "workflow-activation")
            || (a.action == ActivationAction::Activate) != a.target.is_none()
            || a.accepted_at_us != p.operation.accepted_at_us
            || a.sequence != p.operation.sequence
        {
            return Err(StoreError::Integrity(
                "invalid activation projection families or metadata",
            ));
        }
        let owner = self
            .workflow_agreement_owner(
                &p.store,
                &[p.tenant.clone(), p.environment.clone()],
                &a.agreement,
            )
            .await?;
        if owner.as_deref() != Some(&a.customer) {
            return Err(StoreError::DeliveryConflict);
        }
        let accepted:Option<i64>=sqlx::query_scalar("SELECT version FROM workflow_agreement_acceptances WHERE logical_store_id=? AND tenant=? AND environment=? AND customer=? AND agreement_id=? AND binding_id=?").bind(&p.store).bind(&p.tenant).bind(&p.environment).bind(&a.customer).bind(&a.agreement).bind(&a.binding.id).fetch_optional(self.conn()).await?;
        if accepted.is_none() {
            return Err(StoreError::Integrity(
                "activation binding has no acceptance index",
            ));
        }
        self.workflow_agreement_advance_head(
            p,
            &a.customer,
            &a.agreement,
            HeadKind::Activation,
            a.expected_revision,
            &a.head,
        )
        .await?;
        sqlx::query(
            "INSERT INTO workflow_agreement_activations VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&p.store)
        .bind(&p.tenant)
        .bind(&p.environment)
        .bind(&a.agreement)
        .bind(&a.customer)
        .bind(a.head.revision)
        .bind(&a.activation.id)
        .bind(&a.head.transition.id)
        .bind(&a.binding.id)
        .bind(&a.intent.id)
        .bind(a.action.as_str())
        .bind(a.effective_at_us)
        .bind(a.accepted_at_us)
        .bind(a.sequence)
        .bind(a.target.as_ref().map(|r| &r.id))
        .execute(self.conn())
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::sqlite::workflow::Document;
    use serde_json::{json, Value};
    fn document(v: &Value) -> Document {
        Document {
            reference: serde_json::from_value(v["ref"].clone()).unwrap(),
            schema: v["body"]["schema"].as_str().unwrap().into(),
            bytes: ledgerlab_core::canonical::CanonicalBytes::from_value(&v["body"])
                .unwrap()
                .as_slice()
                .to_vec(),
        }
    }
    fn fixture(customer: &str) -> (JournalAppend, ActivationProjection) {
        // Independent encoding operands only. No SQL rows, owner capability,
        // customer assent or complete prepared append is created by this test.
        let f: Value = serde_json::from_slice(include_bytes!(
            "../../../../../service/workflow/agreement/testdata/semantic-independent.json"
        ))
        .unwrap();
        let vectors = f["vectors"].as_array().unwrap();
        let vector = |name: &str| vectors.iter().find(|v| v["name"] == name).unwrap();
        let head = vector(&format!("{customer}.activation-head-v1"));
        let activation = vector(&format!("{customer}.activation-v1"));
        let h = &head["body"];
        let a = &activation["body"];
        let key = &h["operation_key"];
        let reference = |v: &Value| serde_json::from_value::<RecordRef>(v.clone()).unwrap();
        let sequence = h["commit_sequence"].as_str().unwrap().parse().unwrap();
        let at = ledgerlab_core::domain::Timestamp::parse(h["accepted_at"].as_str().unwrap())
            .unwrap()
            .micros();
        let effective =
            ledgerlab_core::domain::Timestamp::parse(a["effective_at"].as_str().unwrap())
                .unwrap()
                .micros();
        let intent = RecordRef {
            kind: "workflow-agreement-intent".into(),
            id: "encoding-only-unused-intent".into(),
            digest: "encoding-only-unused-digest".into(),
        };
        let projection = ActivationProjection {
            customer: h["customer"].as_str().unwrap().into(),
            agreement: h["agreement_id"].as_str().unwrap().into(),
            expected_revision: 0,
            head: AgreementHead {
                revision: 1,
                transition: reference(&head["ref"]),
            },
            activation: reference(&activation["ref"]),
            binding: reference(&h["accepted_binding_ref"]),
            intent: intent.clone(),
            action: ActivationAction::Activate,
            effective_at_us: effective,
            accepted_at_us: at,
            sequence,
            target: None,
        };
        let unused = RecordRef {
            kind: "encoding-only-unused-wrapper".into(),
            id: "encoding-only-unused-wrapper".into(),
            digest: "encoding-only-unused-wrapper".into(),
        };
        let append = JournalAppend {
            store: key["logical_store_id"].as_str().unwrap().into(),
            tenant: key["scope"][0].as_str().unwrap().into(),
            environment: key["scope"][1].as_str().unwrap().into(),
            source: key["source"].as_str().unwrap().into(),
            operation_id: key["operation_id"].as_str().unwrap().into(),
            documents: vec![document(head), document(activation)],
            operation: super::super::JournalOperation {
                identity: vec![],
                kind: "agreement.activate".into(),
                operation_ref: unused.clone(),
                operation_bytes: vec![],
                request_hash: String::new(),
                manifest_ref: unused.clone(),
                manifest_bytes: vec![],
                receipt_ref: unused,
                receipt_bytes: vec![],
                sequence,
                accepted_at_us: at,
                members: vec![],
            },
            semantic_key: vec![],
            alias_of: None,
            projection: super::super::Projection::None,
        };
        (append, projection)
    }
    #[test]
    fn activation_index_copies_match_independent_a_b_records_and_refuse_changed_metadata() {
        for customer in ["A", "B"] {
            let (p, a) = fixture(customer);
            activation_copies(&p, &a).unwrap();
            for mode in 0..6 {
                let mut bad = a.clone();
                match mode {
                    0 => bad.head.revision += 1,
                    1 => bad.sequence += 1,
                    2 => bad.effective_at_us += 1,
                    3 => bad.action = ActivationAction::Revoke,
                    4 => bad.binding.digest = "sha256:wrong-copy".into(),
                    _ => bad.target = Some(bad.activation.clone()),
                }
                assert!(
                    matches!(activation_copies(&p, &bad), Err(StoreError::Integrity(_))),
                    "metadata mismatch {mode} must refuse before a head/index write"
                );
            }
            let mut bad = p;
            let d = &mut bad.documents[0];
            let mut body = ledgerlab_core::canonical::parse(&d.bytes).unwrap();
            body["head_kind"] = json!("agreement_acceptance");
            d.bytes = ledgerlab_core::canonical::CanonicalBytes::from_value(&body)
                .unwrap()
                .as_slice()
                .to_vec();
            assert!(activation_copies(&bad, &a).is_err());
        }
    }
}
