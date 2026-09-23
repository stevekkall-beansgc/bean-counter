use super::*;
use crate::PrincipalContext;
use ledgerlab_core::{
    canonical::outcome as codec,
    domain::{Scope, Timestamp},
};
#[derive(Clone, Debug)]
pub struct OriginalBaseProposal {
    records: Vec<Vec<u8>>,
    rows: Vec<Value>,
}
impl OriginalBaseProposal {
    pub fn from_records(records: Vec<Vec<u8>>) -> Result<Self, ServiceError> {
        require(
            !records.is_empty() && records.len() <= 128,
            "ORIGINAL_BASE_COUNT",
        )?;
        require(
            records.iter().map(Vec::len).sum::<usize>() <= 1_048_576,
            "ORIGINAL_BASE_BYTES",
        )?;
        let rows = records
            .iter()
            .map(|b| {
                require(b.len() <= r3::COMMAND_BYTES, "ORIGINAL_BASE_BYTES")?;
                core(codec::decode(b))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { records, rows })
    }
    pub fn records(&self) -> &[Vec<u8>] {
        &self.records
    }
    fn one(&self, kind: &str) -> Result<&Value, ServiceError> {
        let mut found = self.rows.iter().filter(|r| r["kind"] == kind);
        let row = found.next().ok_or(ServiceError::IntegrityFailure)?;
        require(found.next().is_none(), "ORIGINAL_BASE_KIND")?;
        Ok(row)
    }
    pub(super) fn command(
        &self,
        c: &HostContext,
        head: &Id,
    ) -> Result<outcome::OutcomeCommand, ServiceError> {
        let event = self.one("event")?;
        let evaluation = self.one("base-evaluation")?;
        let e = core(ledgerlab_core::canonical::parse_bounded(
            evaluation["body"]["evaluation_utf8"]
                .as_str()
                .ok_or(ServiceError::IntegrityFailure)?
                .as_bytes(),
            1_048_576,
        ))?;
        let scope: wire::Scope = serde_json::from_value(event["scope"].clone())
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let target = self.one("base-acceptance")?["body"]["target"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?
            .to_owned();
        require(e["event"]["source"] == c.source.as_str(), "ORIGINAL_SOURCE")?;
        let accepted = self.one("target-snapshot")?["body"].clone();
        // Accepted time is pinned inside the exact original target verification. It
        // remains checked by the original decoder and trusted verifier under locks.
        let received = e["received_at"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?;
        let at = accepted["accepted_at"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?;
        Ok(outcome::OutcomeCommand {
            principal: PrincipalContext {
                scope: core(Scope::new(scope.0.as_str(), scope.1.as_str()))?,
                principal_id: c.principal.as_str().into(),
                source: c.source.as_str().into(),
                authority_head: head.as_str().into(),
                can_read: true,
                can_submit: true,
            },
            target,
            invocation_id: e["event"]["invocation_id"]
                .as_str()
                .ok_or(ServiceError::IntegrityFailure)?
                .into(),
            operation: outcome::OutcomeOperation::FinalBase {
                seed: self.records.clone(),
            },
            received_at: core(Timestamp::parse(received))?,
            accepted_at: core(Timestamp::parse(at))?,
            required: self
                .rows
                .iter()
                .filter(|r| r["kind"] == "evidence")
                .map(|r| {
                    Ok(ScopedRecordRef {
                        scope: [scope.0.as_str().into(), scope.1.as_str().into()],
                        kind: "evidence".into(),
                        id: core(r3::canonical_bytes(&r["id"], 4096))?,
                        content_hash: r["content_hash"]
                            .as_str()
                            .ok_or(ServiceError::IntegrityFailure)?
                            .into(),
                    })
                })
                .collect::<Result<_, ServiceError>>()?,
        })
    }
}
/// Read-only locked observations. No SQL handle, authority proof, plan or mutation
/// capability is available to the embedding host's verifier.
pub struct OriginalBaseView<'a> {
    snapshot: &'a OutcomeSnapshot,
}
pub struct OriginalHeadView<'a> {
    pub class: &'static str,
    pub key: &'a [u8],
    pub revision: Option<&'a str>,
    pub value: Option<&'a [u8]>,
}
impl<'a> OriginalBaseView<'a> {
    pub fn records(&self) -> &'a [Vec<u8>] {
        &self.snapshot.records
    }
    pub fn heads(&self) -> Vec<OriginalHeadView<'a>> {
        self.snapshot
            .heads
            .iter()
            .map(|h| OriginalHeadView {
                class: match h.lock.class {
                    OutcomeLockClass::Admission => "admission",
                    OutcomeLockClass::Authority => "authority",
                    OutcomeLockClass::Binding => "binding",
                    OutcomeLockClass::Reservation => "reservation",
                    OutcomeLockClass::Target => "target",
                    OutcomeLockClass::Claim => "claim",
                    OutcomeLockClass::BindingAggregate => "binding-aggregate",
                    OutcomeLockClass::InvocationConsumption => "invocation-consumption",
                    OutcomeLockClass::BaseReversal => "base-reversal",
                },
                key: &h.lock.key,
                revision: h.revision.as_deref(),
                value: h.value.as_deref(),
            })
            .collect()
    }
}
pub struct OriginalVerificationContext<'a> {
    pub principal: &'a str,
    pub source: &'a str,
    pub target: &'a str,
    pub invocation_id: &'a str,
    pub received_at: &'a Timestamp,
    pub accepted_at: &'a Timestamp,
    pub observed_at: &'a Time,
}
/// Only authentication, exact verified terms and finality are host attestations.
/// Rights and the current grant/revision never come from this result.
pub struct OriginalBaseAttestation {
    pub authentication_document: String,
    pub verified_terms: Vec<String>,
    pub finality: bool,
}
pub trait OriginalBaseVerifier: Send + Sync {
    fn verify(
        &self,
        context: OriginalVerificationContext<'_>,
        view: OriginalBaseView<'_>,
    ) -> Result<OriginalBaseAttestation, ServiceError>;
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OriginalPermission {
    Read,
    Submit,
}
#[derive(Clone, Debug)]
pub struct OriginalAuthorityGrant {
    pub principal: Id,
    pub source: r3::types::Source,
    pub grant_document: String,
    pub expected_revision: Option<u64>,
    pub permissions: Vec<OriginalPermission>,
}
#[derive(Clone, Debug)]
pub struct OriginalBinding {
    pub binding_id: Id,
    pub reference: OriginalRecordReference,
    pub expected_revision: Option<u64>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OriginalRecordReference {
    pub kind: String,
    pub id: String,
    pub content_hash: String,
}
#[derive(Clone, Debug)]
pub struct OriginalBaseProvisioning {
    pub evidence: Vec<Vec<u8>>,
    pub grant: OriginalAuthorityGrant,
    pub bindings: Vec<OriginalBinding>,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantAuthorization {
    principal: Id,
    scope: wire::Scope,
    source: r3::types::Source,
    grant: OriginalRecordReference,
    grant_revision: String,
    permissions: Vec<OriginalPermission>,
}
fn reference(row: &Value) -> Result<OriginalRecordReference, ServiceError> {
    serde_json::from_value(codec::reference(row)).map_err(|_| ServiceError::IntegrityFailure)
}
fn rows(raw: &[Vec<u8>]) -> Result<Vec<Value>, ServiceError> {
    raw.iter().map(|b| core(codec::decode(b))).collect()
}
fn document<'a>(rows: &'a [Value], id: &str) -> Result<&'a Value, ServiceError> {
    let mut matching = rows
        .iter()
        .filter(|r| r["kind"] == "evidence" && r["body"]["document_id"] == id);
    let r = matching.next().ok_or(ServiceError::IntegrityFailure)?;
    require(matching.next().is_none(), "ORIGINAL_DOCUMENT")?;
    Ok(r)
}
impl OriginalBaseProvisioning {
    pub(super) async fn install(
        self,
        store: &SqliteStore,
        j: &JournalIdentity,
        head: &Id,
    ) -> Result<(), ServiceError> {
        require(
            self.evidence.len() <= 128
                && self.evidence.iter().map(Vec::len).sum::<usize>() <= 1_048_576
                && self.bindings.len() <= 32,
            "ORIGINAL_PROVISION_BOUND",
        )?;
        let rs = rows(&self.evidence)?;
        for r in &rs {
            require(
                r["kind"] == "evidence" && r["scope"] == json!(j.scope),
                "ORIGINAL_PROVISION_KIND",
            )?;
        }
        let grant = document(&rs, &self.grant.grant_document)?;
        require(grant["body"]["document_type"] == "grant", "ORIGINAL_GRANT")?;
        core(self.grant.principal.validate())?;
        core(self.grant.source.validate())?;
        let revision = self
            .grant
            .expected_revision
            .unwrap_or(0)
            .checked_add(1)
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or(ServiceError::IntegrityFailure)?;
        require(
            !self.grant.permissions.is_empty()
                && self.grant.permissions.len() <= 2
                && self.grant.permissions.windows(2).all(|w| {
                    matches!(
                        (&w[0], &w[1]),
                        (OriginalPermission::Read, OriginalPermission::Submit)
                    )
                }),
            "ORIGINAL_PERMISSIONS",
        )?;
        let auth = GrantAuthorization {
            principal: self.grant.principal,
            scope: j.scope.clone(),
            source: self.grant.source,
            grant: reference(grant)?,
            grant_revision: revision.to_string(),
            permissions: self.grant.permissions,
        };
        let mut heads = vec![(
            OutcomeLock {
                class: OutcomeLockClass::Authority,
                key: core(r3::canonical_bytes(&json!([j.scope, head]), 4096))?,
                mode: OutcomeLockMode::Write,
            },
            self.grant.expected_revision.map(|x| x.to_string()),
            core(r3::canonical_bytes(
                &json!({"active":true,"grant":auth.grant,"authorization":auth}),
                16384,
            ))?,
        )];
        for b in self.bindings {
            core(b.binding_id.validate())?;
            require(b.reference.kind == "binding-snapshot", "ORIGINAL_BINDING")?;
            heads.push((
                OutcomeLock {
                    class: OutcomeLockClass::Binding,
                    key: core(r3::canonical_bytes(&json!([j.scope, b.binding_id]), 4096))?,
                    mode: OutcomeLockMode::Write,
                },
                b.expected_revision.map(|x| x.to_string()),
                core(r3::canonical_bytes(
                    &json!({"active":true,"binding":b.reference}),
                    16384,
                ))?,
            ));
        }
        heads.sort_by(|a, b| (a.0.class, &a.0.key).cmp(&(b.0.class, &b.0.key)));
        store
            .provision_original_authority(&self.evidence, &heads)
            .await
            .map_err(store_error)
    }
}
struct VerifierAdapter<'a> {
    host: &'a RequestHost<'a>,
}
impl outcome::OutcomeAuthority for VerifierAdapter<'_> {
    fn verify(
        &self,
        c: &outcome::OutcomeCommand,
        s: &OutcomeSnapshot,
        write: bool,
    ) -> Result<outcome::AuthorityProof, ServiceError> {
        require(write, "ORIGINAL_WRITE")?;
        let scope = wire::Scope(
            core(Id::parse(c.principal.scope.tenant()))?,
            core(Id::parse(c.principal.scope.environment()))?,
        );
        let key = core(r3::canonical_bytes(
            &json!([scope, c.principal.authority_head]),
            4096,
        ))?;
        let head = s
            .heads
            .iter()
            .find(|h| h.lock.class == OutcomeLockClass::Authority && h.lock.key == key)
            .ok_or(ServiceError::IntegrityFailure)?;
        let hv = core(ledgerlab_core::canonical::parse_bounded(
            head.value
                .as_deref()
                .ok_or(ServiceError::Rejection("ORIGINAL_GRANT_MISSING".into()))?,
            16384,
        ))?;
        require(
            hv.as_object().is_some_and(|o| o.len() == 3) && hv["active"] == true,
            "ORIGINAL_GRANT_AUTHORIZATION",
        )?;
        let a: GrantAuthorization = serde_json::from_value(hv["authorization"].clone())
            .map_err(|_| ServiceError::Rejection("ORIGINAL_GRANT_AUTHORIZATION".into()))?;
        require(
            a.scope == scope
                && a.principal.as_str() == c.principal.principal_id
                && a.source.as_str() == c.principal.source
                && Some(a.grant_revision.as_str()) == head.revision.as_deref()
                && hv["grant"] == json!(a.grant)
                && a.permissions.contains(&OriginalPermission::Read)
                && a.permissions.contains(&OriginalPermission::Submit),
            "ORIGINAL_GRANT_AUTHORIZATION",
        )?;
        let rs = rows(&s.records)?;
        let grant = rs
            .iter()
            .find(|r| codec::reference(r) == json!(a.grant))
            .ok_or(ServiceError::IntegrityFailure)?;
        require(
            grant["kind"] == "evidence"
                && grant["scope"] == json!(scope)
                && grant["body"]["document_type"] == "grant",
            "ORIGINAL_GRANT_AUTHORIZATION",
        )?;
        let att = self.host.host.verifier.verify(
            OriginalVerificationContext {
                principal: &c.principal.principal_id,
                source: &c.principal.source,
                target: &c.target,
                invocation_id: &c.invocation_id,
                received_at: &c.received_at,
                accepted_at: &c.accepted_at,
                observed_at: &self.host.context.observed_at,
            },
            OriginalBaseView { snapshot: s },
        )?;
        require(att.verified_terms.len() <= 128, "ORIGINAL_VERIFIER_BOUND")?;
        let authentication = document(&rs, &att.authentication_document)?;
        require(
            authentication["body"]["document_type"] == "authentication",
            "ORIGINAL_AUTHENTICATION",
        )?;
        for id in &att.verified_terms {
            document(&rs, id)?;
        }
        let mut evidence = rs
            .iter()
            .filter(|r| r["kind"] == "evidence")
            .map(codec::reference)
            .collect::<Vec<_>>();
        evidence.sort_by_cached_key(|v| r3::canonical_bytes(v, 4096).expect("validated reference"));
        Ok(outcome::AuthorityProof {
            scope: [scope.0.as_str().into(), scope.1.as_str().into()],
            target: c.target.clone(),
            invocation_id: c.invocation_id.clone(),
            source: c.principal.source.clone(),
            authority: json!({"principal":a.principal,"grant":a.grant,"grant_revision":a.grant_revision,"active":true,"permissions":a.permissions,"evidence":evidence}),
            authentication: codec::reference(authentication),
            verified_terms: att.verified_terms,
            finality: att.finality,
            authorized_early_close: false,
        })
    }
}
pub(super) async fn prepare(
    host: &RequestHost<'_>,
    tx: &mut SqliteTx,
    terms: &wire::Enroll,
    inputs: &LockedInputs,
) -> Result<FreshBaseAcceptance, ServiceError> {
    let c = host
        .original
        .as_ref()
        .ok_or(ServiceError::Rejection("ORIGINAL_BASE_REQUIRED".into()))?;
    let outcome::OutcomeOperation::FinalBase { seed } = &c.operation else {
        return Err(ServiceError::IntegrityFailure);
    };
    let rs = rows(seed)?;
    let event = rs
        .iter()
        .find(|r| r["kind"] == "event")
        .ok_or(ServiceError::IntegrityFailure)?;
    let q = OutcomeResolve {
        delivery: ScopedDelivery {
            scope: [terms.scope.0.as_str().into(), terms.scope.1.as_str().into()],
            source: c.principal.source.clone(),
            external_id: event["body"]["data"]["external_id"]
                .as_str()
                .ok_or(ServiceError::IntegrityFailure)?
                .into(),
        },
        target: c.target.clone(),
        invocation_id: c.invocation_id.clone(),
        family_key: None,
        required: c.required.clone(),
        locks: outcome::fresh_base_locks(c)?,
    };
    let OutcomeResolution::Complete(snapshot) =
        tx.resolve_outcome(&q).await.map_err(store_error)?
    else {
        return Err(ServiceError::IntegrityFailure);
    };
    let existing = tx
        .lookup_outcome_delivery(&q.delivery)
        .await
        .map_err(store_error)?;
    let plan =
        outcome::prepare_original_base(c, &VerifierAdapter { host }, &snapshot, existing.as_ref())?;
    let at = core(inputs.prefix.ordinal().checked_add(core(Count::new(1))?))?;
    let mut objects = Vec::new();
    for raw in plan.records() {
        core(codec::decode(raw))?;
        let key = r3::raw_sha256(raw);
        objects.push(wire::RetainedObject {
            origin: wire::ObjectOrigin {
                store: terms.store.clone(),
                scope: terms.scope.clone(),
                registration: terms.registration.clone(),
                host: terms.store.clone(),
                ordinal: at,
            },
            kind: wire::FactKind::OriginalBase,
            full_key: wire::RetainedObjectFullKey::V2(core(Id::parse(key.as_str()))?),
            body: b64(raw),
            body_hash: r3::raw_sha256(raw),
            bytes: core(Count::new(raw.len() as u128))?,
        });
    }
    core(FreshBaseAcceptance::from_fresh_v2(plan, terms, objects))
}
pub(super) fn b64(raw: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in raw.chunks(3) {
        s.push(A[(c[0] >> 2) as usize] as char);
        s.push(A[(((c[0] & 3) << 4) | (c.get(1).copied().unwrap_or(0) >> 4)) as usize] as char);
        s.push(if c.len() > 1 {
            A[(((c[1] & 15) << 2) | (c.get(2).copied().unwrap_or(0) >> 6)) as usize] as char
        } else {
            '='
        });
        s.push(if c.len() > 2 {
            A[(c[2] & 63) as usize] as char
        } else {
            '='
        });
    }
    s
}
