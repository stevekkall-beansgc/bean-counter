use crate::domain::{Event, Revision, Roles, Timestamp};
use crate::money::{Decimal, ExactRatio, Money};
use crate::wire::{EventKind, Relation};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Book {
    Retail,
    Supplier,
    CostObservation,
    Allocation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Funding {
    Byok,
    Platform,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Charge,
    Cost,
    Premium,
    Discount,
    Credit,
    Share,
    Allocation,
    Reversal,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Predicate {
    Tier(String),
    Funding(Funding),
    Priority(bool),
    Source(String),
}

/// Only the fixed paths from detailed design §10 are supported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Matcher {
    Direct {
        relation: Relation,
        target: EventKind,
    },
    /// acquired -> published -> optimized, with attributed_to / published_as.
    AcquisitionOptimization,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Price {
    Fixed(Decimal),
    Unit { rate: Decimal, unit: String },
    Percent { percent: Decimal, component: String },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscountMode {
    Additive,
    Sequential,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscountAmount {
    Fixed(Decimal),
    Percent(Decimal),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    Base(Price),
    Premium(Price),
    Discount {
        amount: DiscountAmount,
        component: String,
        mode: DiscountMode,
    },
    /// Proposed typed-only extension: an outcome appends a discount against the
    /// explicitly matched predecessor's booked component. No wire DSL extension.
    LinkedDiscount {
        amount: DiscountAmount,
        component: String,
    },
    Cap {
        ceiling: Decimal,
        stage: String,
        component: String,
    },
    Share {
        percent: Decimal,
        ceiling: Decimal,
        retail_component: String,
    },
    /// Uses retained cost evidence, never an estimate or a payable.
    ObserveCost,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub on: EventKind,
    pub component: String,
    pub when: Vec<Predicate>,
    pub matcher: Option<Matcher>,
    pub operation: Operation,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutcomeTerms {
    pub source: String,
    pub window_us: u64,
    pub report_grace_us: u64,
    pub claim_namespace: String,
}
/// Retained accepted terms, resolved and authenticated by the coordinator.
/// Document IDs are references, not proof that a counterparty assented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub id: String,
    pub agreement: String,
    pub book: Book,
    pub roles: Roles,
    pub assent: String,
    pub offer: Option<String>,
    pub sources: Vec<String>,
    pub event_types: Vec<EventKind>,
    pub unit: String,
    pub maximum_quantity: Decimal,
    pub maximum_exposure: Option<Money>,
    pub outcome: Option<OutcomeTerms>,
    pub correction_sources: Vec<String>,
    pub allowed_modifiers: Vec<String>,
    pub allocation_view: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub binding: Binding,
    pub rules: Vec<Rule>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bundle {
    pub(super) currency: String,
    pub(super) scale: u8,
    pub(super) policies: Vec<Policy>,
    pub(super) order: Vec<(usize, usize)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpectedOperation {
    pub source: String,
    pub operation_id: String,
    pub kind: EventKind,
    pub retail_components: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stage {
    pub id: String,
    pub expected: Vec<ExpectedOperation>,
    pub closure_claim_namespace: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Context {
    pub document: String,
    pub customer: String,
    pub funding: Funding,
    pub tier: Option<String>,
    pub priority: Option<bool>,
    pub stage: Option<Stage>,
}
/// Currently active source/link rights. Supply only after locking/rechecking
/// the authenticated principal's actual grant. Historical terms are separate.
#[derive(Clone, Debug)]
pub struct SourceAuthority {
    pub source: String,
    pub grant: String,
    pub revision: Revision,
    pub active: bool,
    pub event_types: Vec<EventKind>,
    pub relations: Vec<Relation>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub id: String,
    pub binding_id: String,
    pub operation_id: String,
    pub chain: String,
    pub customer: String,
    pub source: String,
    pub unit: String,
    pub maximum_quantity: Decimal,
    pub maximum_exposure: Money,
    pub held: Money,
    pub authorized_at: Timestamp,
    pub start_before: Timestamp,
    pub attested_start: Timestamp,
    pub outcome_deadline: Option<Timestamp>,
    pub completion_event: Option<String>,
}
#[derive(Clone, Debug)]
pub struct CostEvidence {
    pub binding_id: String,
    pub event_id: String,
    pub document: String,
    pub amount: Money,
}
/// Data must be retained/verified by the caller; this function does no IO.
/// `history` is the complete chain history at the locked revision (<=999 events).
pub struct Input<'a> {
    pub event: &'a Event,
    pub context: &'a Context,
    pub history: &'a [Evaluation],
    pub source_authority: &'a SourceAuthority,
    pub invocations: &'a [Invocation],
    pub costs: &'a [CostEvidence],
    pub received_at: &'a Timestamp,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub(super) id: String,
    pub(super) effect_id: String,
    pub(super) obligation_id: String,
    pub(super) binding: Binding,
    pub(super) kind: ActionKind,
    pub(super) book: Book,
    pub(super) component: String,
    pub(super) amount: Money,
    pub(super) sources: Vec<String>,
    pub(super) links: Vec<String>,
    pub(super) inputs: Vec<String>,
    pub(super) reverses: Option<String>,
    pub(super) allocation_parent: Option<String>,
    pub(super) allocation_recipient: Option<String>,
    pub(super) discount_target: Option<String>,
}
impl Action {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn effect_id(&self) -> &str {
        &self.effect_id
    }
    pub fn obligation_id(&self) -> &str {
        &self.obligation_id
    }
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    pub fn kind(&self) -> ActionKind {
        self.kind
    }
    pub fn book(&self) -> Book {
        self.book
    }
    pub fn component(&self) -> &str {
        &self.component
    }
    pub fn amount(&self) -> &Money {
        &self.amount
    }
    pub fn sources(&self) -> &[String] {
        &self.sources
    }
    pub fn links(&self) -> &[String] {
        &self.links
    }
    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }
    pub fn reverses(&self) -> Option<&str> {
        self.reverses.as_deref()
    }
    pub fn allocation_recipient(&self) -> Option<&str> {
        self.allocation_recipient.as_deref()
    }
    pub fn allocation_parent(&self) -> Option<&str> {
        self.allocation_parent.as_deref()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Explanation {
    pub binding_id: String,
    pub rule_id: Option<String>,
    pub code: &'static str,
    pub basis: Option<ExactRatio>,
    pub unrounded: Option<ExactRatio>,
    pub rounded: Option<i128>,
    pub inputs: Vec<String>,
    pub action_ids: Vec<String>,
}
/// Amount grouped only within this decision. Observations/allocations never
/// create payable deltas; a zero net also produces no delta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObligationDelta {
    pub obligation_id: String,
    pub book: Book,
    pub roles: Roles,
    pub amount: Money,
    pub actions: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Consumption {
    pub invocation_id: String,
    pub consume: Money,
    pub release: Money,
}
/// Economic output only, never a receipt, intention, or accepted journal plan.
/// Keep this result as history only when the containing decision really commits.
#[derive(Clone, Debug)]
pub struct Evaluation {
    pub(super) event: Event,
    pub(super) bundle: Bundle,
    pub(super) context: Context,
    pub(super) claim_id: Option<String>,
    pub(super) actions: Vec<Action>,
    pub(super) explanations: Vec<Explanation>,
    pub(super) deltas: Vec<ObligationDelta>,
    pub(super) consumptions: Vec<Consumption>,
    pub(super) invocations: Vec<Invocation>,
    pub(super) closed_stage: Option<String>,
}
impl Evaluation {
    pub fn event(&self) -> &Event {
        &self.event
    }
    pub fn claim_id(&self) -> Option<&str> {
        self.claim_id.as_deref()
    }
    pub fn actions(&self) -> &[Action] {
        &self.actions
    }
    pub fn explanations(&self) -> &[Explanation] {
        &self.explanations
    }
    pub fn deltas(&self) -> &[ObligationDelta] {
        &self.deltas
    }
    pub fn consumptions(&self) -> &[Consumption] {
        &self.consumptions
    }
    pub fn closed_stage(&self) -> Option<&str> {
        self.closed_stage.as_deref()
    }
}
