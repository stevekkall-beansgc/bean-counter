//! Closed point-head values. Collections here are original bounded topology or
//! one obligation's finite slots, never a lifetime case/token/receipt catalog.
use crate::adjudication::{
    commands as w,
    types::{Count, Digest, Id, Time},
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PointKind {
    Enrollment,
    Authority,
    Grant,
    GrantRegistry,
    Token,
    Allocation,
    Receipt,
    Round,
    Gateway,
    Family,
    Case,
    Entitlement,
    Supplier,
    Adjustment,
    Resource,
    Counter,
    VerifiedCursor,
    Delivery,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Point {
    pub kind: PointKind,
    pub key: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentState {
    pub terms: w::Enroll,
    pub enrollment: Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_round: Option<Count>,
    pub last_round: Count,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayState {
    pub namespace: w::Namespace,
    pub epoch: Count,
    pub mode: GatewayMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<Count>,
    pub installed: Count,
    pub acknowledged: Count,
    pub clock_floor: Time,
    pub allocation: Count,
    pub receipt: Count,
    pub allocation_prefix: Count,
    pub receipt_prefix: Count,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatewayMode {
    Open,
    Sealing,
    Sealed,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantState {
    pub grant: w::Grant,
    pub status: GrantStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<Id>,
    pub terminal: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GrantStatus {
    LocalHeld,
    RegisteredUnclaimed,
    Claimed,
    RetiredUnclaimed,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenState {
    pub token: w::Token,
    pub status: TokenStatus,
    pub imported: bool,
    pub reconciled: bool,
    pub advanced: bool,
    pub receipt_advanced: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<w::Receipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<w::Delivery>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submission: Option<w::Submission>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenStatus {
    Issued,
    Active,
    NewCase,
    Alias,
    ReturnedUnused,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseState {
    pub input: w::Submission,
    pub submission: Digest,
    pub receipt: w::Receipt,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission: Option<Count>,
    pub status: CaseStatus,
    pub revision: Count,
    pub signed: crate::adjudication::types::Atoms,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admitted_roles: Option<w::Roles>,
    /// Additional exact evidence; original signed submission and receipt stay immutable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supplements: Vec<w::Evidence>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaseStatus {
    Local,
    OrdinaryPending,
    AdjustmentPending,
    FinalAllow,
    FinalDeny,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyState {
    pub terms: w::FamilyTerms,
    pub closed: bool,
    pub unavailable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_closure: Option<Digest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<Time>,
    pub entitlement: w::EntitlementHead,
    /// Positive original ordinary usage is monotone; a correction never returns it.
    #[serde(default)]
    pub ordinary_positive: Count,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoundGateway {
    pub gateway: Id,
    pub cutoff: Count,
    pub predecessor: Count,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seal: Option<w::Seal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<Digest>,
    pub acknowledged: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoundStatus {
    Draining,
    Ready,
    Committed,
    Aborted,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoundState {
    pub begin: w::Begin,
    pub owner: String,
    pub gateways: Vec<RoundGateway>,
    pub status: RoundStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<Time>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceState {
    pub provisioned: w::Resource,
    pub used: w::Resource,
    pub held: w::Resource,
    pub q: w::Counters,
    pub reserved: w::Counters,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllocationState {
    pub owner: String,
    pub slots: Vec<String>,
    pub held: w::Resource,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "body", deny_unknown_fields)]
pub enum State {
    Preparation(w::PrepareEnroll),
    RoundPreparation(w::PrepareRound),
    Enrollment(Box<EnrollmentState>),
    Gateway(Box<GatewayState>),
    Grant(Box<GrantState>),
    Token(Box<TokenState>),
    Case(Box<CaseState>),
    Family(Box<FamilyState>),
    Round(Box<RoundState>),
    Resource(Box<ResourceState>),
    Allocation(AllocationState),
    Position(Id),
    Supplier(w::Supplier),
    Adjustment(Box<AdjustmentState>),
    Delivery(Box<super::DeliveryState>),
    Authority(Box<AuthorityState>),
    /// Trusted host administrative current authorization, provisioned separately
    /// from first immutable journal introduction. The full source body is kept.
    AuthorityCurrent(w::AuthoritySource),
    Certificate(Box<w::Certificate>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    pub point: Point,
    pub revision: Option<Count>,
    pub state: Option<State>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mutation {
    pub point: Point,
    pub prior: Option<Count>,
    pub revision: Count,
    pub state: State,
}
impl Point {
    pub fn id(kind: PointKind, tag: [u8; 8], id: &str) -> crate::Result<Self> {
        Ok(Self {
            kind,
            key: super::index_key(tag, &[id.as_bytes()])?,
        })
    }
    pub fn position(kind: PointKind, gateway: &Id, n: Count) -> crate::Result<Self> {
        Ok(Self {
            kind,
            key: super::index_key(
                if kind == PointKind::Receipt {
                    *b"RECEIPT_"
                } else {
                    *b"ALLOCATI"
                },
                &[
                    gateway.as_str().as_bytes(),
                    n.value().to_string().as_bytes(),
                ],
            )?,
        })
    }
    pub fn family(f: &w::Family) -> crate::Result<Self> {
        Ok(Self {
            kind: PointKind::Family,
            key: super::family_key(f)?,
        })
    }
    pub fn case(c: &w::Case) -> crate::Result<Self> {
        Ok(Self {
            kind: PointKind::Case,
            key: super::case_key(c)?,
        })
    }
    pub fn delivery(d: &w::Delivery) -> crate::Result<Self> {
        Ok(Self {
            kind: PointKind::Delivery,
            key: super::delivery_key(d)?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityState {
    pub source: w::AuthoritySource,
    pub origin: w::ObjectOrigin,
    pub segment: Digest,
}

impl CaseState {
    /// A committed family certificate transfers pending interpretation without
    /// rewriting N case rows. Accepted/denied facts remain final.
    pub fn effective_status(&self, family: &FamilyState) -> CaseStatus {
        if self.status == CaseStatus::OrdinaryPending && family.unavailable {
            CaseStatus::AdjustmentPending
        } else {
            self.status
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdjustmentState {
    pub terms: w::Pool,
    pub funding_used: Count,
    pub positive_used: Count,
    pub negative_used: Count,
    pub gross_used: Count,
}
