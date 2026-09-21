//! Private, storage-facing projections supplied by the coordinator after validation.
//!
//! Canonical bytes and hashes are retained verbatim. This boundary cannot authorize
//! a decision or validate a policy: the coordinator must verify canonical hashes,
//! IDs, body/index agreement, schema, references and the complete plan first.

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Scope {
    pub tenant: String,
    pub environment: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalRecord {
    pub canonical_bytes: Vec<u8>,
    pub content_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JournalRecord {
    pub scope: Scope,
    pub canonical: CanonicalRecord,
    pub row: JournalRow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum JournalRow {
    Document {
        id: String,
        kind: String,
    },
    Party {
        id: String,
        role_metadata_doc: String,
    },
    SourceGrant {
        id: String,
        principal_id: String,
        source: String,
        grant_doc: String,
    },
    Binding {
        id: String,
        agreement_id: String,
        version: i64,
        policy_doc: String,
        assent_doc: String,
        roles_doc: String,
        context_doc: String,
        currency: String,
        scale: i64,
    },
    Event {
        id: String,
        source: String,
        external_id: String,
        operation_id: String,
        kind: String,
        chain_id: String,
        decision_id: String,
        ingress_hash: String,
        claim_facts_hash: String,
        ingress_bytes: Vec<u8>,
        occurred_us: Option<i64>,
        received_us: i64,
    },
    Snapshot {
        id: String,
        event_id: String,
        document_id: String,
        purpose: String,
    },
    DeliveryKey {
        source: String,
        external_id: String,
        ingress_hash: String,
        canonical_event_id: String,
        kind: String,
        observed_us: i64,
    },
    Claim {
        id: String,
        source: String,
        operation_id: String,
        kind: String,
        token: String,
        facts_hash: String,
        event_id: String,
    },
    Effect {
        id: String,
        agreement_id: String,
        component: String,
        claim_id: String,
        namespace: String,
        facts_hash: String,
        action_id: String,
        match_key_bytes: Vec<u8>,
    },
    Action {
        id: String,
        event_id: String,
        decision_id: String,
        effect_id: String,
        obligation_id: String,
        kind: String,
        book: String,
        component: String,
        binding_id: String,
        snapshot_doc: String,
        roles_doc: String,
        currency: String,
        scale: i64,
        atoms: String,
        reverses: Option<String>,
        allocation_parent: Option<String>,
    },
    ActionSource {
        action_id: String,
        event_id: String,
    },
    ActionDependency {
        action_id: String,
        input_action_id: String,
    },
    Explanation {
        id: String,
        event_id: String,
        ordinal: i64,
        code: String,
        rule_id: Option<String>,
    },
    Intention {
        id: String,
        event_id: String,
        obligation_id: String,
        destination_id: String,
        idempotency_key: String,
    },
    ControlTransition {
        id: String,
        control_kind: String,
        control_id: String,
        from_revision: i64,
        to_revision: i64,
        from_event_count: i64,
        to_event_count: i64,
        event_id: String,
        document_id: String,
    },
    ChainRevision {
        chain_id: String,
        revision: i64,
        event_id: String,
        decision_id: String,
    },
    Manifest {
        id: String,
        event_id: String,
        chain_id: String,
        revision: i64,
        decision_hash: String,
    },
    Receipt {
        id: String,
        event_id: String,
        decision_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Installation {
    pub scope: Scope,
    pub logical_store_id: String,
    pub mode: String,
    pub admission: String,
    pub dispatch_hold: bool,
    pub dispatch_enabled: bool,
    pub generation: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Chain {
    pub scope: Scope,
    pub id: String,
    pub customer: String,
    pub currency: String,
    pub scale: i64,
    pub binding_set_doc: String,
    pub context_doc: String,
    pub revision: i64,
    pub event_count: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AuthorityHead {
    pub scope: Scope,
    pub id: String,
    pub grant_id: String,
    pub revision: i64,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BindingHead {
    pub scope: Scope,
    pub id: String,
    pub selector_doc: String,
    pub binding_id: String,
    pub revision: i64,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChainAdvance {
    pub scope: Scope,
    pub id: String,
    pub from_revision: i64,
    pub to_revision: i64,
    pub from_event_count: i64,
    pub to_event_count: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HeldDelivery {
    pub scope: Scope,
    pub intention_id: String,
    pub next_attempt_us: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StoredIdentity {
    pub event_id: String,
    pub ingress_hash: String,
    pub receipt: CanonicalRecord,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StoredClaim {
    pub event_id: String,
    pub facts_hash: String,
    pub receipt: CanonicalRecord,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StoredDelivery {
    pub state: String,
    pub attempts: i64,
    pub generation: i64,
    pub next_attempt_us: i64,
    pub lease_owner: Option<String>,
    pub lease_until_us: Option<i64>,
    pub last_observation: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum WriteOp {
    Journal(Box<JournalRecord>),
    SeedInstallation(Installation),
    SeedChain(Chain),
    SeedAuthority(AuthorityHead),
    SeedBinding(BindingHead),
    AdvanceChain(ChainAdvance),
    HoldDelivery(HeldDelivery),
}
