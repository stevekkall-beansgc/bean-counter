//! Named item-level hooks: batching may not collapse these boundaries.
use crate::oracle::{check, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Edge {
    Before,
    After,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Boundary {
    Write {
        name: String,
        item: usize,
        edge: Edge,
    },
    BeforeCommitSend,
    CommitInFlight,
    AfterCommitAcknowledged,
    EvaluatorAfterBase,
    Await {
        name: String,
        phase: CommitPhase,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CommitPhase {
    Before,
    InFlight,
    Acknowledged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    Rollback,
    EvaluationInvalid,
    EvaluationOverflow,
    LoseReply,
    /// Suppress commit outcome AND primary lookup until the first call returns.
    UnknownCommit {
        durable: bool,
    },
    /// Hold the old transaction at a bounded test barrier while a primary lookup
    /// sees no committed identity. The service must keep its outcome unknown.
    UnknownCommitActive,
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Injection {
    pub boundary: Boundary,
    pub fault: Fault,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub injection: Injection,
    pub hits: usize,
}

impl Hit {
    pub fn verify(&self, expected: &Injection) -> Result<()> {
        check(
            &self.injection == expected && self.hits == 1,
            "requested failpoint did not fire exactly once",
        )
    }
}

/// Complete adapter hook catalogue. Rollback awaits need their triggering fault.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CancellationPoint {
    pub name: String,
    pub class: AwaitClass,
    pub phase: CommitPhase,
    pub trigger: Option<Injection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AwaitClass {
    Begin,
    Lock,
    Read,
    Write { name: String, item: usize },
    Commit,
    Rollback,
    Cleanup,
}
