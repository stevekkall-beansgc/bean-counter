//! Closed R3 wire types transcribed from the accepted schema; no economics here.
//! Public DTO construction is untrusted. Use checked parsing before planning.
use super::types::*;
use super::{check_set, check_unique, ensure, Validate};
use crate::Result;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope(pub Id, pub Id);
impl Validate for Scope {
    fn validate(&self) -> Result<()> {
        self.0.validate()?;
        self.1.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Family(pub Scope, pub Id, pub Id, pub Id);
impl Validate for Family {
    fn validate(&self) -> Result<()> {
        self.0.validate()?;
        self.1.validate()?;
        self.2.validate()?;
        self.3.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Case(pub Family, pub Source, pub Id);
impl Validate for Case {
    fn validate(&self) -> Result<()> {
        self.0.validate()?;
        self.1.validate()?;
        self.2.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivery(pub Scope, pub Source, pub Id);
impl Validate for Delivery {
    fn validate(&self) -> Result<()> {
        self.0.validate()?;
        self.1.validate()?;
        self.2.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roles {
    pub provider: Id,
    pub cost_originator: Id,
    pub bearer: Id,
    pub payer: Id,
    pub beneficiary: Id,
    pub recipient: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payer_delegation: Option<Digest>,
}
impl Validate for Roles {
    fn validate(&self) -> Result<()> {
        self.provider.validate()?;
        self.cost_originator.validate()?;
        self.bearer.validate()?;
        self.payer.validate()?;
        self.beneficiary.validate()?;
        self.recipient.validate()?;
        if let Some(value) = &self.payer_delegation {
            value.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub body: String,
    pub sha256: Digest,
}
impl Validate for Evidence {
    fn validate(&self) -> Result<()> {
        ensure(
            self.body.chars().count() <= 5464,
            "SHAPE",
            "maximum string length",
        )?;
        super::proofs::decode_base64(&self.body, 4096)?;
        self.sha256.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NamespaceRoute {
    #[serde(rename = "full-case-sha256-mod/1")]
    FullCaseSha256Mod1,
}
impl Validate for NamespaceRoute {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Namespace {
    pub scope: Scope,
    pub tag: String,
    pub gateway: Id,
    pub route: NamespaceRoute,
}
impl Validate for Namespace {
    fn validate(&self) -> Result<()> {
        self.scope.validate()?;
        ensure(is_hex(&self.tag, 32), "SHAPE", "namespace tag")?;
        self.gateway.validate()?;
        self.route.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Head {
    pub store: Id,
    pub scope: Scope,
    pub target: Id,
    pub enrollment: Digest,
    pub ordinal: Count,
    pub segment: Digest,
    pub root: Digest,
}
impl Validate for Head {
    fn validate(&self) -> Result<()> {
        self.store.validate()?;
        self.scope.validate()?;
        self.target.validate()?;
        self.enrollment.validate()?;
        self.ordinal.validate()?;
        self.segment.validate()?;
        self.root.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorityPermission {
    #[serde(rename = "enroll")]
    Enroll,
    #[serde(rename = "capacity")]
    Capacity,
    #[serde(rename = "submit")]
    Submit,
    #[serde(rename = "read")]
    Read,
    #[serde(rename = "decide")]
    Decide,
    #[serde(rename = "adjust")]
    Adjust,
    #[serde(rename = "correct")]
    Correct,
    #[serde(rename = "close")]
    Close,
    #[serde(rename = "replace")]
    Replace,
}
impl Validate for AuthorityPermission {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    pub principal: Id,
    pub permission: AuthorityPermission,
    pub document: Digest,
    pub revision: Count,
    pub observed_at: Time,
    pub command: Digest,
    pub head: Digest,
}
impl Validate for Authority {
    fn validate(&self) -> Result<()> {
        self.principal.validate()?;
        self.permission.validate()?;
        self.document.validate()?;
        self.revision.validate()?;
        self.observed_at.validate()?;
        self.command.validate()?;
        self.head.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FamilyTermsBook {
    #[serde(rename = "RETAIL")]
    Retail,
    #[serde(rename = "SUPPLIER")]
    Supplier,
}
impl Validate for FamilyTermsBook {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyTerms {
    pub book: FamilyTermsBook,
    pub key: Family,
    pub prerequisites: Vec<u64>,
    pub ordinary_atoms: Atoms,
    pub correction_atoms: Vec<Atoms>,
    pub source: Source,
    pub supplier_pool: Id,
    pub roles: Roles,
    pub assent: Digest,
    pub starts_at: Time,
    pub occurs_before: Time,
    pub received_by: Time,
    pub accepted_by: Time,
    pub correction_by: Time,
}
impl Validate for FamilyTerms {
    fn validate(&self) -> Result<()> {
        self.book.validate()?;
        self.key.validate()?;
        ensure(
            (0..=31).contains(&self.prerequisites.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.prerequisites {
            ensure((0..=31).contains(item), "SHAPE", "integer range")?;
        }
        check_set(&self.prerequisites)?;
        self.ordinary_atoms.validate()?;
        ensure(
            (1..=32).contains(&self.correction_atoms.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.correction_atoms {
            item.validate()?;
        }
        check_set(&self.correction_atoms)?;
        self.source.validate()?;
        self.supplier_pool.validate()?;
        self.roles.validate()?;
        self.assent.validate()?;
        self.starts_at.validate()?;
        self.occurs_before.validate()?;
        self.received_by.validate()?;
        self.accepted_by.validate()?;
        self.correction_by.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Supplier {
    pub id: Id,
    pub maximum: Count,
    pub consumed: Count,
    pub held: Count,
    pub released: Count,
}
impl Validate for Supplier {
    fn validate(&self) -> Result<()> {
        self.id.validate()?;
        self.maximum.validate()?;
        self.consumed.validate()?;
        self.held.validate()?;
        self.released.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PoolAuthorizationDirection {
    #[serde(rename = "POSITIVE")]
    Positive,
    #[serde(rename = "NEGATIVE")]
    Negative,
    #[serde(rename = "ZERO")]
    Zero,
}
impl Validate for PoolAuthorizationDirection {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoolAuthorization {
    pub direction: PoolAuthorizationDirection,
    pub roles: Roles,
    pub assent: Digest,
}
impl Validate for PoolAuthorization {
    fn validate(&self) -> Result<()> {
        self.direction.validate()?;
        self.roles.validate()?;
        self.assent.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pool {
    pub id: Id,
    pub funding: Count,
    pub positive: Count,
    pub negative: Count,
    pub gross: Count,
    pub authorizations: Vec<PoolAuthorization>,
}
impl Validate for Pool {
    fn validate(&self) -> Result<()> {
        self.id.validate()?;
        self.funding.validate()?;
        self.positive.validate()?;
        self.negative.validate()?;
        self.gross.validate()?;
        ensure(
            (1..=3).contains(&self.authorizations.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.authorizations {
            item.validate()?;
        }
        check_set(&self.authorizations)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityKey(pub Id, pub Id, pub Count);
impl Validate for AuthorityKey {
    fn validate(&self) -> Result<()> {
        self.0.validate()?;
        self.1.validate()?;
        self.2.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegatedRoles {
    pub provider: Id,
    pub cost_originator: Id,
    pub bearer: Id,
    pub payer: Id,
    pub beneficiary: Id,
    pub recipient: Id,
}
impl Validate for DelegatedRoles {
    fn validate(&self) -> Result<()> {
        self.provider.validate()?;
        self.cost_originator.validate()?;
        self.bearer.validate()?;
        self.payer.validate()?;
        self.beneficiary.validate()?;
        self.recipient.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthoritySourceBodyAuthorizationPermissionsItem {
    #[serde(rename = "enroll")]
    Enroll,
    #[serde(rename = "capacity")]
    Capacity,
    #[serde(rename = "submit")]
    Submit,
    #[serde(rename = "read")]
    Read,
    #[serde(rename = "decide")]
    Decide,
    #[serde(rename = "adjust")]
    Adjust,
    #[serde(rename = "correct")]
    Correct,
    #[serde(rename = "close")]
    Close,
    #[serde(rename = "replace")]
    Replace,
}
impl Validate for AuthoritySourceBodyAuthorizationPermissionsItem {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoritySourceBodyDelegationAssent {
    pub accepted_at: Time,
    pub terms: Digest,
}
impl Validate for AuthoritySourceBodyDelegationAssent {
    fn validate(&self) -> Result<()> {
        self.accepted_at.validate()?;
        self.terms.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum AuthoritySourceBody {
    #[serde(rename = "AUTHORIZATION")]
    Authorization {
        source: Id,
        id: Id,
        revision: Count,
        scope: Scope,
        target: Id,
        principal: Id,
        permissions: Vec<AuthoritySourceBodyAuthorizationPermissionsItem>,
        starts_at: Time,
        ends_at: Time,
    },
    #[serde(rename = "ASSENT")]
    Assent {
        source: Id,
        id: Id,
        revision: Count,
        scope: Scope,
        target: Id,
        roles: Roles,
        terms: Digest,
    },
    #[serde(rename = "DELEGATION")]
    Delegation {
        source: Id,
        id: Id,
        revision: Count,
        scope: Scope,
        target: Id,
        roles: DelegatedRoles,
        agreement_ids: Vec<Id>,
        maximum_exposure: Count,
        starts_at: Time,
        ends_at: Time,
        acceptor: Id,
        assent: AuthoritySourceBodyDelegationAssent,
    },
}
impl Validate for AuthoritySourceBody {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Authorization {
                source,
                id,
                revision,
                scope,
                target,
                principal,
                permissions,
                starts_at,
                ends_at,
            } => {
                source.validate()?;
                id.validate()?;
                revision.validate()?;
                scope.validate()?;
                target.validate()?;
                principal.validate()?;
                ensure((1..=9).contains(&permissions.len()), "SHAPE", "array bound")?;
                for item in permissions {
                    item.validate()?;
                }
                check_set(permissions)?;
                starts_at.validate()?;
                ends_at.validate()?;
            }
            Self::Assent {
                source,
                id,
                revision,
                scope,
                target,
                roles,
                terms,
            } => {
                source.validate()?;
                id.validate()?;
                revision.validate()?;
                scope.validate()?;
                target.validate()?;
                roles.validate()?;
                terms.validate()?;
            }
            Self::Delegation {
                source,
                id,
                revision,
                scope,
                target,
                roles,
                agreement_ids,
                maximum_exposure,
                starts_at,
                ends_at,
                acceptor,
                assent,
            } => {
                source.validate()?;
                id.validate()?;
                revision.validate()?;
                scope.validate()?;
                target.validate()?;
                roles.validate()?;
                ensure(
                    (1..=32).contains(&agreement_ids.len()),
                    "SHAPE",
                    "array bound",
                )?;
                for item in agreement_ids {
                    item.validate()?;
                }
                check_set(agreement_ids)?;
                maximum_exposure.validate()?;
                starts_at.validate()?;
                ends_at.validate()?;
                acceptor.validate()?;
                assent.validate()?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoritySource {
    pub body: String,
    pub body_hash: Digest,
    pub bytes: Count,
}
impl Validate for AuthoritySource {
    fn validate(&self) -> Result<()> {
        ensure(
            self.body.chars().count() <= 21848,
            "SHAPE",
            "maximum string length",
        )?;
        super::proofs::decode_base64(&self.body, 16384)?;
        self.body_hash.validate()?;
        self.bytes.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub canonical_bytes: Count,
    pub trusted_bytes: Count,
    pub records: Count,
    pub index_pages: Count,
    pub index_values: Count,
    pub workspace_bytes: Count,
}
impl Validate for Resource {
    fn validate(&self) -> Result<()> {
        self.canonical_bytes.validate()?;
        self.trusted_bytes.validate()?;
        self.records.validate()?;
        self.index_pages.validate()?;
        self.index_values.validate()?;
        self.workspace_bytes.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counters {
    pub segment: Count,
    pub head_revision: Count,
    pub grant: Count,
    pub grant_registry: Count,
    pub allocation: Count,
    pub receipt: Count,
    pub control: Count,
    pub round: Count,
    pub r#import: Count,
    pub terminal: Count,
    pub allocation_prefix: Count,
    pub receipt_prefix: Count,
    pub index_cardinality: Count,
    pub writer_epoch: Count,
    pub economic_revision: Count,
    pub resource_revision: Count,
}
impl Validate for Counters {
    fn validate(&self) -> Result<()> {
        self.segment.validate()?;
        self.head_revision.validate()?;
        self.grant.validate()?;
        self.grant_registry.validate()?;
        self.allocation.validate()?;
        self.receipt.validate()?;
        self.control.validate()?;
        self.round.validate()?;
        self.r#import.validate()?;
        self.terminal.validate()?;
        self.allocation_prefix.validate()?;
        self.receipt_prefix.validate()?;
        self.index_cardinality.validate()?;
        self.writer_epoch.validate()?;
        self.economic_revision.validate()?;
        self.resource_revision.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GrantTemplate {
    #[serde(rename = "complete-ingress/1")]
    CompleteIngress1,
}
impl Validate for GrantTemplate {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub id: Id,
    pub store: Id,
    pub registration: Id,
    pub gateway: Id,
    pub namespace: Namespace,
    pub template: GrantTemplate,
    pub resources: Resource,
    pub counters: Counters,
    pub journal_head: Digest,
    pub authentication: Digest,
}
impl Validate for Grant {
    fn validate(&self) -> Result<()> {
        self.id.validate()?;
        self.store.validate()?;
        self.registration.validate()?;
        self.gateway.validate()?;
        self.namespace.validate()?;
        self.template.validate()?;
        self.resources.validate()?;
        self.counters.validate()?;
        self.journal_head.validate()?;
        self.authentication.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FactKind {
    #[serde(rename = "ENROLLMENT")]
    Enrollment,
    #[serde(rename = "ENROLL_PREPARATION")]
    EnrollPreparation,
    #[serde(rename = "ROUND_PREPARATION")]
    RoundPreparation,
    #[serde(rename = "GRANT")]
    Grant,
    #[serde(rename = "CLAIM")]
    Claim,
    #[serde(rename = "RECEIPT")]
    Receipt,
    #[serde(rename = "ALIAS")]
    Alias,
    #[serde(rename = "RETURNED_UNUSED")]
    ReturnedUnused,
    #[serde(rename = "RETIREMENT")]
    Retirement,
    #[serde(rename = "RECONCILIATION")]
    Reconciliation,
    #[serde(rename = "INSTALLATION")]
    Installation,
    #[serde(rename = "ORIGINAL_BASE")]
    OriginalBase,
    #[serde(rename = "AUTHORITY")]
    Authority,
    #[serde(rename = "BEGIN")]
    Begin,
    #[serde(rename = "SEAL")]
    Seal,
    #[serde(rename = "TERMINAL")]
    Terminal,
}
impl Validate for FactKind {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProofFullKey {
    V0(Delivery),
    V1(Case),
    V2(Id),
}
impl Validate for ProofFullKey {
    fn validate(&self) -> Result<()> {
        match self {
            Self::V0(v) => v.validate()?,
            Self::V1(v) => v.validate()?,
            Self::V2(v) => v.validate()?,
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proof {
    pub store: Id,
    pub scope: Scope,
    pub registration: Id,
    pub host: Id,
    pub ordinal: Count,
    pub segment: Digest,
    pub root: Digest,
    pub fact_kind: FactKind,
    pub full_key: ProofFullKey,
    pub body_hash: Digest,
    pub bytes: Count,
    pub trusted_observation_ref: Digest,
}
impl Validate for Proof {
    fn validate(&self) -> Result<()> {
        self.store.validate()?;
        self.scope.validate()?;
        self.registration.validate()?;
        self.host.validate()?;
        self.ordinal.validate()?;
        self.segment.validate()?;
        self.root.validate()?;
        self.fact_kind.validate()?;
        self.full_key.validate()?;
        self.body_hash.validate()?;
        self.bytes.validate()?;
        self.trusted_observation_ref.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectOrigin {
    pub store: Id,
    pub scope: Scope,
    pub registration: Id,
    pub host: Id,
    pub ordinal: Count,
}
impl Validate for ObjectOrigin {
    fn validate(&self) -> Result<()> {
        self.store.validate()?;
        self.scope.validate()?;
        self.registration.validate()?;
        self.host.validate()?;
        self.ordinal.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RetainedObjectFullKey {
    V0(Delivery),
    V1(Case),
    V2(Id),
    V3(AuthorityKey),
}
impl Validate for RetainedObjectFullKey {
    fn validate(&self) -> Result<()> {
        match self {
            Self::V0(v) => v.validate()?,
            Self::V1(v) => v.validate()?,
            Self::V2(v) => v.validate()?,
            Self::V3(v) => v.validate()?,
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedObject {
    pub origin: ObjectOrigin,
    pub kind: FactKind,
    pub full_key: RetainedObjectFullKey,
    pub body: String,
    pub body_hash: Digest,
    pub bytes: Count,
}
impl Validate for RetainedObject {
    fn validate(&self) -> Result<()> {
        self.origin.validate()?;
        self.kind.validate()?;
        self.full_key.validate()?;
        ensure(
            self.body.chars().count() <= 349528,
            "SHAPE",
            "maximum string length",
        )?;
        super::proofs::decode_base64(&self.body, 262144)?;
        self.body_hash.validate()?;
        self.bytes.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenCategory {
    #[serde(rename = "ORDINARY")]
    Ordinary,
    #[serde(rename = "ADJUSTMENT")]
    Adjustment,
}
impl Validate for TokenCategory {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Token {
    pub id: Id,
    pub grant: Id,
    pub gateway: Id,
    pub allocation: Count,
    pub category: TokenCategory,
    pub claim: Digest,
}
impl Validate for Token {
    fn validate(&self) -> Result<()> {
        self.id.validate()?;
        self.grant.validate()?;
        self.gateway.validate()?;
        self.allocation.validate()?;
        self.category.validate()?;
        self.claim.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub case: Case,
    pub delivery: Delivery,
    pub submission: Digest,
    pub token: Id,
    pub gateway: Id,
    pub epoch: Count,
    pub position: Count,
    pub received_at: Time,
    pub journal_head: Digest,
}
impl Validate for Receipt {
    fn validate(&self) -> Result<()> {
        self.case.validate()?;
        self.delivery.validate()?;
        self.submission.validate()?;
        self.token.validate()?;
        self.gateway.validate()?;
        self.epoch.validate()?;
        self.position.validate()?;
        self.received_at.validate()?;
        self.journal_head.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", deny_unknown_fields)]
pub enum Coverage {
    #[serde(rename = "COMPLETE_GATEWAY_CUTOFF")]
    CompleteGatewayCutoff {
        gateway: Id,
        cutoff: Count,
        allocation_prefix: Count,
        receipt_high: Count,
        receipt_prefix: Count,
        disposition_root: Digest,
        receipt_root: Digest,
        observation: Digest,
    },
    #[serde(rename = "UNRECONCILED")]
    Unreconciled { gateway: Id, observation: Digest },
    #[serde(rename = "UNKNOWN_GATEWAY_COVERAGE")]
    UnknownGatewayCoverage { gateway: Id },
}
impl Validate for Coverage {
    fn validate(&self) -> Result<()> {
        match self {
            Self::CompleteGatewayCutoff {
                gateway,
                cutoff,
                allocation_prefix,
                receipt_high,
                receipt_prefix,
                disposition_root,
                receipt_root,
                observation,
            } => {
                gateway.validate()?;
                cutoff.validate()?;
                allocation_prefix.validate()?;
                receipt_high.validate()?;
                receipt_prefix.validate()?;
                disposition_root.validate()?;
                receipt_root.validate()?;
                observation.validate()?;
            }
            Self::Unreconciled {
                gateway,
                observation,
            } => {
                gateway.validate()?;
                observation.validate()?;
            }
            Self::UnknownGatewayCoverage { gateway } => {
                gateway.validate()?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Submission {
    pub case: Case,
    pub occurred_at: Time,
    pub evidence: Vec<Evidence>,
    pub sender_backfill: bool,
}
impl Validate for Submission {
    fn validate(&self) -> Result<()> {
        self.case.validate()?;
        self.occurred_at.validate()?;
        ensure(
            (0..=16).contains(&self.evidence.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.evidence {
            item.validate()?;
        }
        check_set(&self.evidence)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionBook {
    #[serde(rename = "RETAIL")]
    Retail,
    #[serde(rename = "SUPPLIER")]
    Supplier,
}
impl Validate for ActionBook {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionKind {
    #[serde(rename = "ORDINARY")]
    Ordinary,
    #[serde(rename = "ADJUSTMENT")]
    Adjustment,
    #[serde(rename = "INVERSE")]
    Inverse,
    #[serde(rename = "REPLACEMENT")]
    Replacement,
}
impl Validate for ActionKind {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub book: ActionBook,
    pub case: Case,
    pub revision: Count,
    pub kind: ActionKind,
    pub signed_atoms: Atoms,
    pub magnitude: Count,
    pub roles: Roles,
    pub assent: Digest,
}
impl Validate for Action {
    fn validate(&self) -> Result<()> {
        self.book.validate()?;
        self.case.validate()?;
        self.revision.validate()?;
        self.kind.validate()?;
        self.signed_atoms.validate()?;
        self.magnitude.validate()?;
        self.roles.validate()?;
        self.assent.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", deny_unknown_fields)]
pub enum EntitlementHead {
    #[serde(rename = "UNCONSUMED")]
    Unconsumed {},
    #[serde(rename = "CONSUMED")]
    Consumed {
        case: Box<Case>,
        revision: Count,
        head: Digest,
    },
}
impl Validate for EntitlementHead {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Unconsumed {} => {}
            Self::Consumed {
                case,
                revision,
                head,
            } => {
                case.validate()?;
                revision.validate()?;
                head.validate()?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyHead {
    pub family: Family,
    pub terms: Digest,
    pub closed: bool,
    pub unavailable: bool,
    pub entitlement: EntitlementHead,
}
impl Validate for FamilyHead {
    fn validate(&self) -> Result<()> {
        self.family.validate()?;
        self.terms.validate()?;
        self.entitlement.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Certificate {
    pub predecessor: Digest,
    pub enrollment: Digest,
    pub family_heads: Vec<FamilyHead>,
    pub families: Vec<Family>,
    pub unavailable: Vec<Family>,
    pub supplier_before: Vec<Supplier>,
    pub supplier_after: Vec<Supplier>,
    pub round: Count,
    pub cutoffs: Vec<Coverage>,
    pub closed_at: Time,
}
impl Validate for Certificate {
    fn validate(&self) -> Result<()> {
        self.predecessor.validate()?;
        self.enrollment.validate()?;
        ensure(
            (1..=32).contains(&self.family_heads.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.family_heads {
            item.validate()?;
        }
        check_set(&self.family_heads)?;
        ensure(
            (1..=32).contains(&self.families.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.families {
            item.validate()?;
        }
        check_set(&self.families)?;
        ensure(
            (0..=32).contains(&self.unavailable.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.unavailable {
            item.validate()?;
        }
        check_set(&self.unavailable)?;
        ensure(
            (0..=8).contains(&self.supplier_before.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.supplier_before {
            item.validate()?;
        }
        check_set(&self.supplier_before)?;
        ensure(
            (0..=8).contains(&self.supplier_after.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.supplier_after {
            item.validate()?;
        }
        check_set(&self.supplier_after)?;
        self.round.validate()?;
        ensure(
            (0..=4).contains(&self.cutoffs.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.cutoffs {
            item.validate()?;
        }
        check_set(&self.cutoffs)?;
        self.closed_at.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareEnroll {
    pub store: Id,
    pub scope: Scope,
    pub registration: Id,
    pub gateway: Id,
    pub namespace: Namespace,
    pub intent: Digest,
}
impl Validate for PrepareEnroll {
    fn validate(&self) -> Result<()> {
        self.store.validate()?;
        self.scope.validate()?;
        self.registration.validate()?;
        self.gateway.validate()?;
        self.namespace.validate()?;
        self.intent.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrepareRoundMode {
    #[serde(rename = "CANCELLABLE")]
    Cancellable,
}
impl Validate for PrepareRoundMode {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareRound {
    pub round: Count,
    pub predecessor: Count,
    pub gateway: Id,
    pub mode: PrepareRoundMode,
    pub enrollment: Digest,
    pub proof: Proof,
}
impl Validate for PrepareRound {
    fn validate(&self) -> Result<()> {
        self.round.validate()?;
        self.predecessor.validate()?;
        self.gateway.validate()?;
        self.mode.validate()?;
        self.enrollment.validate()?;
        self.proof.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enroll {
    pub preparations: Vec<Proof>,
    pub registration: Id,
    pub store: Id,
    pub scope: Scope,
    pub target: Id,
    pub base_receipt: Digest,
    pub base_manifest: Digest,
    pub base_atoms: Count,
    pub supplier_booked: Count,
    pub premium_cap: Count,
    pub families: Vec<FamilyTerms>,
    pub gateways: Vec<Namespace>,
    pub suppliers: Vec<Supplier>,
    pub pools: Vec<Pool>,
}
impl Validate for Enroll {
    fn validate(&self) -> Result<()> {
        ensure(
            (1..=4).contains(&self.preparations.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.preparations {
            item.validate()?;
        }
        check_set(&self.preparations)?;
        self.registration.validate()?;
        self.store.validate()?;
        self.scope.validate()?;
        self.target.validate()?;
        self.base_receipt.validate()?;
        self.base_manifest.validate()?;
        self.base_atoms.validate()?;
        self.supplier_booked.validate()?;
        self.premium_cap.validate()?;
        ensure(
            (1..=32).contains(&self.families.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.families {
            item.validate()?;
        }
        check_unique(&self.families)?;
        ensure(
            (1..=4).contains(&self.gateways.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.gateways {
            item.validate()?;
        }
        check_unique(&self.gateways)?;
        ensure(
            (0..=8).contains(&self.suppliers.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.suppliers {
            item.validate()?;
        }
        check_set(&self.suppliers)?;
        ensure((0..=3).contains(&self.pools.len()), "SHAPE", "array bound")?;
        for item in &self.pools {
            item.validate()?;
        }
        check_set(&self.pools)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalGrant {
    pub grant: Grant,
    pub proof: Proof,
}
impl Validate for LocalGrant {
    fn validate(&self) -> Result<()> {
        self.grant.validate()?;
        self.proof.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterGrant {
    pub grant: Grant,
    pub proof: Proof,
}
impl Validate for RegisterGrant {
    fn validate(&self) -> Result<()> {
        self.grant.validate()?;
        self.proof.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Issue {
    pub grant: Id,
    pub token: Token,
}
impl Validate for Issue {
    fn validate(&self) -> Result<()> {
        self.grant.validate()?;
        self.token.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Activate {
    pub token: Id,
    pub gateway: Id,
    pub proof: Proof,
}
impl Validate for Activate {
    fn validate(&self) -> Result<()> {
        self.token.validate()?;
        self.gateway.validate()?;
        self.proof.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receive {
    pub token: Id,
    pub gateway: Id,
    pub epoch: Count,
    pub delivery: Delivery,
    pub submission: Submission,
    pub received_at: Time,
}
impl Validate for Receive {
    fn validate(&self) -> Result<()> {
        self.token.validate()?;
        self.gateway.validate()?;
        self.epoch.validate()?;
        self.delivery.validate()?;
        self.submission.validate()?;
        self.received_at.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnUnused {
    pub token: Id,
    pub gateway: Id,
    pub claim: Digest,
    pub proof: Proof,
}
impl Validate for ReturnUnused {
    fn validate(&self) -> Result<()> {
        self.token.validate()?;
        self.gateway.validate()?;
        self.claim.validate()?;
        self.proof.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportReceipt {
    pub token: Id,
    pub proof: Proof,
}
impl Validate for ImportReceipt {
    fn validate(&self) -> Result<()> {
        self.token.validate()?;
        self.proof.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reconcile {
    pub token: Id,
    pub proof: Proof,
}
impl Validate for Reconcile {
    fn validate(&self) -> Result<()> {
        self.token.validate()?;
        self.proof.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Advance {
    pub gateway: Id,
    pub through: Count,
}
impl Validate for Advance {
    fn validate(&self) -> Result<()> {
        self.gateway.validate()?;
        self.through.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdvanceReceipt {
    pub gateway: Id,
    pub through: Count,
}
impl Validate for AdvanceReceipt {
    fn validate(&self) -> Result<()> {
        self.gateway.validate()?;
        self.through.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalTerminal {
    pub grant: Id,
    pub gateway: Id,
    pub proof: Proof,
}
impl Validate for LocalTerminal {
    fn validate(&self) -> Result<()> {
        self.grant.validate()?;
        self.gateway.validate()?;
        self.proof.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetireGrant {
    pub grant: Id,
}
impl Validate for RetireGrant {
    fn validate(&self) -> Result<()> {
        self.grant.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BeginMode {
    #[serde(rename = "FINISH_ONLY")]
    FinishOnly,
    #[serde(rename = "CANCELLABLE")]
    Cancellable,
}
impl Validate for BeginMode {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Begin {
    pub preparations: Vec<Proof>,
    pub round: Count,
    pub predecessor: Count,
    pub mode: BeginMode,
    pub families: Vec<Family>,
    pub gateways: Vec<Id>,
}
impl Validate for Begin {
    fn validate(&self) -> Result<()> {
        ensure(
            (0..=4).contains(&self.preparations.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.preparations {
            item.validate()?;
        }
        check_set(&self.preparations)?;
        self.round.validate()?;
        self.predecessor.validate()?;
        self.mode.validate()?;
        ensure(
            (1..=32).contains(&self.families.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.families {
            item.validate()?;
        }
        check_set(&self.families)?;
        ensure(
            (0..=4).contains(&self.gateways.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.gateways {
            item.validate()?;
        }
        check_set(&self.gateways)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealBegin {
    pub round: Count,
    pub gateway: Id,
    pub predecessor: Count,
    pub proof: Proof,
}
impl Validate for SealBegin {
    fn validate(&self) -> Result<()> {
        self.round.validate()?;
        self.gateway.validate()?;
        self.predecessor.validate()?;
        self.proof.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sealed {
    pub round: Count,
    pub gateway: Id,
}
impl Validate for Sealed {
    fn validate(&self) -> Result<()> {
        self.round.validate()?;
        self.gateway.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drain {
    pub round: Count,
    pub gateway: Id,
    pub proof: Proof,
}
impl Validate for Drain {
    fn validate(&self) -> Result<()> {
        self.round.validate()?;
        self.gateway.validate()?;
        self.proof.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ready {
    pub round: Count,
}
impl Validate for Ready {
    fn validate(&self) -> Result<()> {
        self.round.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Close {
    pub round: Count,
    pub closed_at: Time,
}
impl Validate for Close {
    fn validate(&self) -> Result<()> {
        self.round.validate()?;
        self.closed_at.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Abort {
    pub round: Count,
}
impl Validate for Abort {
    fn validate(&self) -> Result<()> {
        self.round.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallOutcome {
    #[serde(rename = "COMMITTED")]
    Committed,
    #[serde(rename = "ABORTED")]
    Aborted,
}
impl Validate for InstallOutcome {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Install {
    pub round: Count,
    pub gateway: Id,
    pub outcome: InstallOutcome,
    pub proof: Proof,
    pub begin: Proof,
}
impl Validate for Install {
    fn validate(&self) -> Result<()> {
        self.round.validate()?;
        self.gateway.validate()?;
        self.outcome.validate()?;
        self.proof.validate()?;
        self.begin.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AckInstall {
    pub round: Count,
    pub gateway: Id,
    pub proof: Proof,
}
impl Validate for AckInstall {
    fn validate(&self) -> Result<()> {
        self.round.validate()?;
        self.gateway.validate()?;
        self.proof.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Supplement {
    pub case: Case,
    pub evidence: Vec<Evidence>,
}
impl Validate for Supplement {
    fn validate(&self) -> Result<()> {
        self.case.validate()?;
        ensure(
            (1..=16).contains(&self.evidence.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.evidence {
            item.validate()?;
        }
        check_set(&self.evidence)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecideVerdict {
    #[serde(rename = "ALLOW")]
    Allow,
    #[serde(rename = "DENY")]
    Deny,
}
impl Validate for DecideVerdict {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecidePath {
    #[serde(rename = "ORDINARY")]
    Ordinary,
    #[serde(rename = "ADJUSTMENT")]
    Adjustment,
}
impl Validate for DecidePath {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decide {
    pub case: Case,
    pub verdict: DecideVerdict,
    pub path: DecidePath,
    pub signed_atoms: Atoms,
    pub pool: Id,
    pub roles: Roles,
    pub assent: Digest,
    pub reason: String,
}
impl Validate for Decide {
    fn validate(&self) -> Result<()> {
        self.case.validate()?;
        self.verdict.validate()?;
        self.path.validate()?;
        self.signed_atoms.validate()?;
        self.pool.validate()?;
        self.roles.validate()?;
        self.assent.validate()?;
        ensure(
            self.reason.chars().count() >= 1,
            "SHAPE",
            "minimum string length",
        )?;
        ensure(self.reason.len() <= 256, "SHAPE", "UTF-8 byte limit")?;
        ensure(
            !self
                .reason
                .chars()
                .any(|c| (c as u32) < 32 || c == '\u{7f}'),
            "SHAPE",
            "control character",
        )?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Correct {
    pub case: Case,
    pub expected_revision: Count,
    pub replacement: Atoms,
    pub roles: Roles,
    pub assent: Digest,
}
impl Validate for Correct {
    fn validate(&self) -> Result<()> {
        self.case.validate()?;
        self.expected_revision.validate()?;
        self.replacement.validate()?;
        self.roles.validate()?;
        self.assent.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaceWriter {
    pub gateway: Id,
    pub old_epoch: Count,
    pub new_epoch: Count,
    pub journal_head: Digest,
    pub fence: Digest,
}
impl Validate for ReplaceWriter {
    fn validate(&self) -> Result<()> {
        self.gateway.validate()?;
        self.old_epoch.validate()?;
        self.new_epoch.validate()?;
        self.journal_head.validate()?;
        self.fence.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtendResources {
    pub host: Id,
    pub resources: Resource,
}
impl Validate for ExtendResources {
    fn validate(&self) -> Result<()> {
        self.host.validate()?;
        self.resources.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Command {
    #[serde(rename = "PREPARE_ENROLL")]
    PrepareEnroll {
        key: Delivery,
        payload: PrepareEnroll,
        authority: Authority,
    },
    #[serde(rename = "PREPARE_ROUND")]
    PrepareRound {
        key: Delivery,
        payload: PrepareRound,
        authority: Authority,
    },
    #[serde(rename = "ENROLL")]
    Enroll {
        key: Delivery,
        payload: Enroll,
        authority: Authority,
    },
    #[serde(rename = "LOCAL_GRANT")]
    LocalGrant {
        key: Delivery,
        payload: LocalGrant,
        authority: Authority,
    },
    #[serde(rename = "REGISTER_GRANT")]
    RegisterGrant {
        key: Delivery,
        payload: RegisterGrant,
        authority: Authority,
    },
    #[serde(rename = "ISSUE")]
    Issue {
        key: Delivery,
        payload: Issue,
        authority: Authority,
    },
    #[serde(rename = "ACTIVATE")]
    Activate {
        key: Delivery,
        payload: Activate,
        authority: Authority,
    },
    #[serde(rename = "RECEIVE")]
    Receive {
        key: Delivery,
        payload: Receive,
        authority: Authority,
    },
    #[serde(rename = "RETURN_UNUSED")]
    ReturnUnused {
        key: Delivery,
        payload: ReturnUnused,
        authority: Authority,
    },
    #[serde(rename = "IMPORT")]
    Import {
        key: Delivery,
        payload: ImportReceipt,
        authority: Authority,
    },
    #[serde(rename = "RECONCILE")]
    Reconcile {
        key: Delivery,
        payload: Reconcile,
        authority: Authority,
    },
    #[serde(rename = "ADVANCE")]
    Advance {
        key: Delivery,
        payload: Advance,
        authority: Authority,
    },
    #[serde(rename = "ADVANCE_RECEIPT")]
    AdvanceReceipt {
        key: Delivery,
        payload: AdvanceReceipt,
        authority: Authority,
    },
    #[serde(rename = "LOCAL_TERMINAL")]
    LocalTerminal {
        key: Delivery,
        payload: LocalTerminal,
        authority: Authority,
    },
    #[serde(rename = "RETIRE_GRANT")]
    RetireGrant {
        key: Delivery,
        payload: RetireGrant,
        authority: Authority,
    },
    #[serde(rename = "BEGIN")]
    Begin {
        key: Delivery,
        payload: Begin,
        authority: Authority,
    },
    #[serde(rename = "SEAL_BEGIN")]
    SealBegin {
        key: Delivery,
        payload: SealBegin,
        authority: Authority,
    },
    #[serde(rename = "SEALED")]
    Sealed {
        key: Delivery,
        payload: Sealed,
        authority: Authority,
    },
    #[serde(rename = "DRAIN")]
    Drain {
        key: Delivery,
        payload: Drain,
        authority: Authority,
    },
    #[serde(rename = "READY")]
    Ready {
        key: Delivery,
        payload: Ready,
        authority: Authority,
    },
    #[serde(rename = "CLOSE")]
    Close {
        key: Delivery,
        payload: Close,
        authority: Authority,
    },
    #[serde(rename = "ABORT")]
    Abort {
        key: Delivery,
        payload: Abort,
        authority: Authority,
    },
    #[serde(rename = "INSTALL")]
    Install {
        key: Delivery,
        payload: Install,
        authority: Authority,
    },
    #[serde(rename = "ACK_INSTALL")]
    AckInstall {
        key: Delivery,
        payload: AckInstall,
        authority: Authority,
    },
    #[serde(rename = "SUPPLEMENT")]
    Supplement {
        key: Delivery,
        payload: Supplement,
        authority: Authority,
    },
    #[serde(rename = "DECIDE")]
    Decide {
        key: Delivery,
        payload: Decide,
        authority: Authority,
    },
    #[serde(rename = "CORRECT")]
    Correct {
        key: Delivery,
        payload: Correct,
        authority: Authority,
    },
    #[serde(rename = "REPLACE_WRITER")]
    ReplaceWriter {
        key: Delivery,
        payload: ReplaceWriter,
        authority: Authority,
    },
    #[serde(rename = "EXTEND_RESOURCES")]
    ExtendResources {
        key: Delivery,
        payload: ExtendResources,
        authority: Authority,
    },
}
impl Validate for Command {
    fn validate(&self) -> Result<()> {
        match self {
            Self::PrepareEnroll {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::PrepareRound {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::Enroll {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::LocalGrant {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::RegisterGrant {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::Issue {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::Activate {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::Receive {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::ReturnUnused {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::Import {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::Reconcile {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::Advance {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::AdvanceReceipt {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::LocalTerminal {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::RetireGrant {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::Begin {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::SealBegin {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::Sealed {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::Drain {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::Ready {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::Close {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::Abort {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::Install {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::AckInstall {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::Supplement {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::Decide {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::Correct {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::ReplaceWriter {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
            Self::ExtendResources {
                key,
                payload,
                authority,
            } => {
                key.validate()?;
                payload.validate()?;
                authority.validate()?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoundBeginMode {
    #[serde(rename = "FINISH_ONLY")]
    FinishOnly,
    #[serde(rename = "CANCELLABLE")]
    Cancellable,
}
impl Validate for RoundBeginMode {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoundBeginCutoffsItem {
    pub gateway: Id,
    pub cutoff: Count,
    pub gateway_predecessor: Count,
}
impl Validate for RoundBeginCutoffsItem {
    fn validate(&self) -> Result<()> {
        self.gateway.validate()?;
        self.cutoff.validate()?;
        self.gateway_predecessor.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoundBegin {
    pub round: Count,
    pub predecessor: Count,
    pub mode: RoundBeginMode,
    pub cutoffs: Vec<RoundBeginCutoffsItem>,
}
impl Validate for RoundBegin {
    fn validate(&self) -> Result<()> {
        self.round.validate()?;
        self.predecessor.validate()?;
        self.mode.validate()?;
        ensure(
            (0..=4).contains(&self.cutoffs.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.cutoffs {
            item.validate()?;
        }
        check_set(&self.cutoffs)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seal {
    pub round: Count,
    pub gateway: Id,
    pub cutoff: Count,
    pub receipt_high: Count,
    pub disposition_root: Digest,
    pub receipt_root: Digest,
}
impl Validate for Seal {
    fn validate(&self) -> Result<()> {
        self.round.validate()?;
        self.gateway.validate()?;
        self.cutoff.validate()?;
        self.receipt_high.validate()?;
        self.disposition_root.validate()?;
        self.receipt_root.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Effect {
    #[serde(rename = "RECEIPT")]
    Receipt { body: Receipt },
    #[serde(rename = "ACTION")]
    Action { body: Action },
    #[serde(rename = "CLOSURE")]
    Closure { body: Certificate },
    #[serde(rename = "ROUND_BEGIN")]
    RoundBegin { body: RoundBegin },
    #[serde(rename = "SEAL")]
    Seal { body: Seal },
}
impl Validate for Effect {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Receipt { body } => {
                body.validate()?;
            }
            Self::Action { body } => {
                body.validate()?;
            }
            Self::Closure { body } => {
                body.validate()?;
            }
            Self::RoundBegin { body } => {
                body.validate()?;
            }
            Self::Seal { body } => {
                body.validate()?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandResultStatus {
    #[serde(rename = "COMMITTED")]
    Committed,
    #[serde(rename = "DUPLICATE")]
    Duplicate,
    #[serde(rename = "REFUSED")]
    Refused,
    #[serde(rename = "UNKNOWN")]
    Unknown,
}
impl Validate for CommandResultStatus {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandResult {
    pub status: CommandResultStatus,
    pub code: String,
    pub effects: Vec<Effect>,
    pub root: Digest,
}
impl Validate for CommandResult {
    fn validate(&self) -> Result<()> {
        self.status.validate()?;
        ensure(
            self.code.chars().count() >= 1,
            "SHAPE",
            "minimum string length",
        )?;
        ensure(self.code.len() <= 64, "SHAPE", "UTF-8 byte limit")?;
        ensure(
            !self.code.chars().any(|c| (c as u32) < 32 || c == '\u{7f}'),
            "SHAPE",
            "control character",
        )?;
        ensure(
            (0..=128).contains(&self.effects.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.effects {
            item.validate()?;
        }
        self.root.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SegmentProfile {
    #[serde(rename = "central-adjudication-r3/1")]
    CentralAdjudicationR31,
}
impl Validate for SegmentProfile {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub host: Id,
    pub profile: SegmentProfile,
    pub ordinal: Count,
    pub previous: Digest,
    pub previous_root: Digest,
    pub command: Command,
    pub result: CommandResult,
    pub dependencies: Vec<Digest>,
    pub objects: Vec<RetainedObject>,
}
impl Validate for Segment {
    fn validate(&self) -> Result<()> {
        self.host.validate()?;
        self.profile.validate()?;
        self.ordinal.validate()?;
        self.previous.validate()?;
        self.previous_root.validate()?;
        self.command.validate()?;
        self.result.validate()?;
        ensure(
            (0..=128).contains(&self.dependencies.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.dependencies {
            item.validate()?;
        }
        check_set(&self.dependencies)?;
        ensure(
            (0..=216).contains(&self.objects.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.objects {
            item.validate()?;
        }
        check_set(&self.objects)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetryResponseKnowledge {
    #[serde(rename = "AUTHORITATIVE_AT_PREFIX")]
    AuthoritativeAtPrefix,
    #[serde(rename = "CACHED_VERIFIED_PREFIX")]
    CachedVerifiedPrefix,
    #[serde(rename = "UNKNOWN")]
    Unknown,
}
impl Validate for RetryResponseKnowledge {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetryResponseCurrentLifecycle {
    #[serde(rename = "UNKNOWN")]
    Unknown,
    #[serde(rename = "ORDINARY_PENDING")]
    OrdinaryPending,
    #[serde(rename = "ADJUSTMENT_PENDING")]
    AdjustmentPending,
    #[serde(rename = "FINAL_ALLOW")]
    FinalAllow,
    #[serde(rename = "FINAL_DENY")]
    FinalDeny,
}
impl Validate for RetryResponseCurrentLifecycle {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetryResponseCentralAdmission {
    #[serde(rename = "UNKNOWN")]
    Unknown,
    #[serde(rename = "PRESENT_AT_PREFIX")]
    PresentAtPrefix,
    #[serde(rename = "ABSENT_AT_PREFIX")]
    AbsentAtPrefix,
}
impl Validate for RetryResponseCentralAdmission {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryResponse {
    pub receipt: Receipt,
    pub knowledge: RetryResponseKnowledge,
    pub current_lifecycle: RetryResponseCurrentLifecycle,
    pub central_admission: RetryResponseCentralAdmission,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<Head>,
    pub coverage: Vec<Coverage>,
}
impl Validate for RetryResponse {
    fn validate(&self) -> Result<()> {
        self.receipt.validate()?;
        self.knowledge.validate()?;
        self.current_lifecycle.validate()?;
        self.central_admission.validate()?;
        if let Some(value) = &self.prefix {
            value.validate()?;
        }
        ensure(
            (0..=4).contains(&self.coverage.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.coverage {
            item.validate()?;
        }
        check_set(&self.coverage)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExpectedPrefixProfile {
    #[serde(rename = "central-adjudication-r3/1")]
    CentralAdjudicationR31,
}
impl Validate for ExpectedPrefixProfile {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedPrefix {
    pub store: Id,
    pub scope: Scope,
    pub target: Id,
    pub profile: ExpectedPrefixProfile,
    pub enrollment: Digest,
    pub registration: Id,
    pub host: Id,
    pub ordinal: Count,
    pub segment: Digest,
    pub root: Digest,
}
impl Validate for ExpectedPrefix {
    fn validate(&self) -> Result<()> {
        self.store.validate()?;
        self.scope.validate()?;
        self.target.validate()?;
        self.profile.validate()?;
        self.enrollment.validate()?;
        self.registration.validate()?;
        self.host.validate()?;
        self.ordinal.validate()?;
        self.segment.validate()?;
        self.root.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadBudget {
    pub bytes: Count,
    pub pages: Count,
    pub segments: Count,
}
impl Validate for ReadBudget {
    fn validate(&self) -> Result<()> {
        self.bytes.validate()?;
        self.pages.validate()?;
        self.segments.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadCursor {
    pub expected: ExpectedPrefix,
    pub ordinal: Count,
    pub byte_offset: Count,
    pub verified_root: Digest,
    pub continuation: Digest,
}
impl Validate for ReadCursor {
    fn validate(&self) -> Result<()> {
        self.expected.validate()?;
        self.ordinal.validate()?;
        self.byte_offset.validate()?;
        self.verified_root.validate()?;
        self.continuation.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadRequest {
    pub expected: ExpectedPrefix,
    pub budget: ReadBudget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<ReadCursor>,
}
impl Validate for ReadRequest {
    fn validate(&self) -> Result<()> {
        self.expected.validate()?;
        self.budget.validate()?;
        if let Some(value) = &self.cursor {
            value.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReadResponseCompleteSelection {
    #[serde(rename = "HISTORICAL_PREFIX")]
    HistoricalPrefix,
    #[serde(rename = "CURRENT_AT_READ")]
    CurrentAtRead,
}
impl Validate for ReadResponseCompleteSelection {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReadResponseCompleteScope {
    #[serde(rename = "CENTRAL_PREFIX")]
    CentralPrefix,
    #[serde(rename = "GATEWAY_PREFIX")]
    GatewayPrefix,
}
impl Validate for ReadResponseCompleteScope {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", deny_unknown_fields)]
pub enum ReadResponse {
    #[serde(rename = "COMPLETE")]
    Complete {
        expected: ExpectedPrefix,
        measured: ReadBudget,
        selection: ReadResponseCompleteSelection,
        scope: ReadResponseCompleteScope,
        coverage: Vec<Coverage>,
    },
    #[serde(rename = "INCOMPLETE")]
    Incomplete {
        cursor: ReadCursor,
        measured: ReadBudget,
    },
    #[serde(rename = "ERROR")]
    Error { code: String },
}
impl Validate for ReadResponse {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Complete {
                expected,
                measured,
                selection,
                scope,
                coverage,
            } => {
                expected.validate()?;
                measured.validate()?;
                selection.validate()?;
                scope.validate()?;
                ensure((1..=4).contains(&coverage.len()), "SHAPE", "array bound")?;
                for item in coverage {
                    item.validate()?;
                }
                check_set(coverage)?;
            }
            Self::Incomplete { cursor, measured } => {
                cursor.validate()?;
                measured.validate()?;
            }
            Self::Error { code } => {
                ensure(code.chars().count() >= 1, "SHAPE", "minimum string length")?;
                ensure(code.len() <= 64, "SHAPE", "UTF-8 byte limit")?;
                ensure(
                    !code.chars().any(|c| (c as u32) < 32 || c == '\u{7f}'),
                    "SHAPE",
                    "control character",
                )?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SealCursorPhase {
    #[serde(rename = "DISPOSITIONS")]
    Dispositions,
    #[serde(rename = "RECEIPTS")]
    Receipts,
    #[serde(rename = "COMPLETE")]
    Complete,
}
impl Validate for SealCursorPhase {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealCursor {
    pub expected: ExpectedPrefix,
    pub round: Count,
    pub phase: SealCursorPhase,
    pub entry: Count,
    pub byte_offset: Count,
}
impl Validate for SealCursor {
    fn validate(&self) -> Result<()> {
        self.expected.validate()?;
        self.round.validate()?;
        self.phase.validate()?;
        self.entry.validate()?;
        self.byte_offset.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealBudget {
    pub bytes: Count,
    pub pages: Count,
}
impl Validate for SealBudget {
    fn validate(&self) -> Result<()> {
        self.bytes.validate()?;
        self.pages.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", deny_unknown_fields)]
pub enum SealReadResponse {
    #[serde(rename = "INCOMPLETE")]
    Incomplete {
        cursor: SealCursor,
        measured: SealBudget,
    },
    #[serde(rename = "COMPLETE")]
    Complete {
        cursor: SealCursor,
        measured: SealBudget,
        disposition_root: Digest,
        receipt_root: Digest,
    },
}
impl Validate for SealReadResponse {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Incomplete { cursor, measured } => {
                cursor.validate()?;
                measured.validate()?;
            }
            Self::Complete {
                cursor,
                measured,
                disposition_root,
                receipt_root,
            } => {
                cursor.validate()?;
                measured.validate()?;
                disposition_root.validate()?;
                receipt_root.validate()?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonPolicy {
    pub resolution_atoms: Count,
}
impl Validate for ComparisonPolicy {
    fn validate(&self) -> Result<()> {
        self.resolution_atoms.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonRequest {
    pub expected: ExpectedPrefix,
    pub policy: ComparisonPolicy,
    pub budget: ReadBudget,
    pub coverage: Vec<Coverage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<ReadCursor>,
}
impl Validate for ComparisonRequest {
    fn validate(&self) -> Result<()> {
        self.expected.validate()?;
        self.policy.validate()?;
        self.budget.validate()?;
        ensure(
            (1..=4).contains(&self.coverage.len()),
            "SHAPE",
            "array bound",
        )?;
        for item in &self.coverage {
            item.validate()?;
        }
        check_set(&self.coverage)?;
        if let Some(value) = &self.cursor {
            value.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", deny_unknown_fields)]
pub enum ComparisonResponse {
    #[serde(rename = "COMPARABLE")]
    Comparable {
        expected: ExpectedPrefix,
        actual: Atoms,
        alternative: Atoms,
        difference: Atoms,
        supplier_booked: Count,
        coverage: Vec<Coverage>,
        measured: ReadBudget,
    },
    #[serde(rename = "UNSUPPORTED")]
    Unsupported {
        reason: String,
        measured: ReadBudget,
    },
    #[serde(rename = "POLICY_FAILURE")]
    PolicyFailure {
        at_case: Case,
        reason: String,
        measured: ReadBudget,
    },
    #[serde(rename = "INCOMPLETE")]
    Incomplete {
        expected: ExpectedPrefix,
        reason: String,
        cursor: Box<ReadCursor>,
        measured: ReadBudget,
    },
}
impl Validate for ComparisonResponse {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Comparable {
                expected,
                actual,
                alternative,
                difference,
                supplier_booked,
                coverage,
                measured,
            } => {
                expected.validate()?;
                actual.validate()?;
                alternative.validate()?;
                difference.validate()?;
                supplier_booked.validate()?;
                ensure((1..=4).contains(&coverage.len()), "SHAPE", "array bound")?;
                for item in coverage {
                    item.validate()?;
                }
                check_set(coverage)?;
                measured.validate()?;
            }
            Self::Unsupported { reason, measured } => {
                ensure(
                    reason.chars().count() >= 1,
                    "SHAPE",
                    "minimum string length",
                )?;
                ensure(reason.len() <= 256, "SHAPE", "UTF-8 byte limit")?;
                ensure(
                    !reason.chars().any(|c| (c as u32) < 32 || c == '\u{7f}'),
                    "SHAPE",
                    "control character",
                )?;
                measured.validate()?;
            }
            Self::PolicyFailure {
                at_case,
                reason,
                measured,
            } => {
                at_case.validate()?;
                ensure(
                    reason.chars().count() >= 1,
                    "SHAPE",
                    "minimum string length",
                )?;
                ensure(reason.len() <= 256, "SHAPE", "UTF-8 byte limit")?;
                ensure(
                    !reason.chars().any(|c| (c as u32) < 32 || c == '\u{7f}'),
                    "SHAPE",
                    "control character",
                )?;
                measured.validate()?;
            }
            Self::Incomplete {
                expected,
                reason,
                cursor,
                measured,
            } => {
                expected.validate()?;
                ensure(
                    reason.chars().count() >= 1,
                    "SHAPE",
                    "minimum string length",
                )?;
                ensure(reason.len() <= 256, "SHAPE", "UTF-8 byte limit")?;
                ensure(
                    !reason.chars().any(|c| (c as u32) < 32 || c == '\u{7f}'),
                    "SHAPE",
                    "control character",
                )?;
                cursor.validate()?;
                measured.validate()?;
            }
        }
        Ok(())
    }
}
