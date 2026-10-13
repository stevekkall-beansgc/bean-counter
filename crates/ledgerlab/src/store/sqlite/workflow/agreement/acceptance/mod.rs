//! Mechanical agreement ownership, independent heads and immutable candidates.
//! Indexed rows never certify assent, grants, schedule selection or origin.
use super::{JournalAppend, RecordRef, SqliteTx, StoreError};
use sqlx::Row;
use tokio::time::timeout_at;

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
impl SqliteTx {
    pub(crate) async fn workflow_agreement_owner(
        &mut self,
        store: &str,
        scope: &[String; 2],
        agreement: &str,
    ) -> Result<Option<String>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result=timeout_at(self.deadline,async {self.workflow_schema().await?;sqlx::query_scalar("SELECT customer FROM workflow_agreement_owners WHERE logical_store_id=? AND tenant=? AND environment=? AND agreement_id=?").bind(store).bind(&scope[0]).bind(&scope[1]).bind(agreement).fetch_optional(self.conn()).await.map_err(StoreError::from)}).await.map_err(|_|StoreError::Deadline)?;
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
        let result=timeout_at(self.deadline,async {self.workflow_schema().await?;let row=sqlx::query("SELECT h.revision,d.kind AS head_kind,d.id AS head_id,d.digest AS head_digest FROM workflow_agreement_heads h LEFT JOIN workflow_documents d ON d.id=h.transition_id WHERE h.logical_store_id=? AND h.tenant=? AND h.environment=? AND h.customer=? AND h.agreement_id=? AND h.head_kind=?").bind(store).bind(&scope[0]).bind(&scope[1]).bind(customer).bind(agreement).bind(kind.as_str()).fetch_optional(self.conn()).await?;row.map(|r|Ok::<_,StoreError>(AgreementHead{revision:r.try_get("revision")?,transition:row_ref(&r,"head")?})).transpose().map_err(StoreError::from)}).await.map_err(|_|StoreError::Deadline)?;
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
        let result=timeout_at(self.deadline,async {self.workflow_schema().await?;let row=sqlx::query("SELECT d.kind AS accepted_kind,d.id AS accepted_id,d.digest AS accepted_digest FROM workflow_agreement_acceptances a JOIN workflow_documents b ON b.id=a.binding_id JOIN workflow_documents d ON d.id=a.accepted_id WHERE a.logical_store_id=? AND a.tenant=? AND a.environment=? AND b.kind=? AND b.id=? AND b.digest=?").bind(store).bind(&scope[0]).bind(&scope[1]).bind(&binding.kind).bind(&binding.id).bind(&binding.digest).fetch_optional(self.conn()).await?;row.map(|r|row_ref(&r,"accepted")).transpose().map_err(StoreError::from)}).await.map_err(|_|StoreError::Deadline)?;
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
        let result=timeout_at(self.deadline,async {
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
