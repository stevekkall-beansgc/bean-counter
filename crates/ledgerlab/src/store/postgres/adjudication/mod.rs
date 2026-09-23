//! Native byte-preserving R3 PostgreSQL projections. No physical admission or
//! CommitCapability is implemented here; reads and storage primitives do not
//! certify reserved PGDATA/WAL/temp backing or writer recovery.
mod locks;
mod persist;
mod read;
pub(super) mod recovery;
mod resolve;
#[cfg(test)]
mod tests;
use crate::{
    service::accept::adjudication::{
        TrustedJournalHead, ValidatedAdjudicationPlan, VerifiedSource,
    },
    store::{adjudication::*, errors::StoreError},
};
use ledgerlab_core::adjudication::{
    self as r3, commands as wire, runtime,
    types::{Count, Digest},
    Validate,
};
pub(super) use locks::Locked;
use read::{head, saved, source};
use serde_json::json;
use tokio_postgres::GenericClient;
fn invalid() -> StoreError {
    StoreError::Integrity("native R3 PostgreSQL projection")
}
fn core(_: ledgerlab_core::Error) -> StoreError {
    invalid()
}
fn journal_key(j: &JournalIdentity) -> Result<Vec<u8>, StoreError> {
    runtime::index_key(
        *b"r3host01",
        &[
            j.store.as_str().as_bytes(),
            j.scope.0.as_str().as_bytes(),
            j.scope.1.as_str().as_bytes(),
            j.registration.as_str().as_bytes(),
            j.host.as_str().as_bytes(),
        ],
    )
    .map_err(core)
}
fn ordinal(raw: Vec<u8>) -> Result<Count, StoreError> {
    Count::new(u128::from_be_bytes(raw.try_into().map_err(|_| invalid())?)).map_err(core)
}
fn number(n: Count) -> Vec<u8> {
    n.value().to_be_bytes().to_vec()
}
fn tag(k: HeadKind) -> i32 {
    match k {
        HeadKind::Enrollment => 0,
        HeadKind::Authority => 1,
        HeadKind::Grant => 2,
        HeadKind::GrantRegistry => 3,
        HeadKind::Token => 4,
        HeadKind::Allocation => 5,
        HeadKind::Receipt => 6,
        HeadKind::Round => 7,
        HeadKind::Gateway => 8,
        HeadKind::Family => 9,
        HeadKind::Case => 10,
        HeadKind::Entitlement => 11,
        HeadKind::Supplier => 12,
        HeadKind::Adjustment => 13,
        HeadKind::Resource => 14,
        HeadKind::Counter => 15,
        HeadKind::VerifiedCursor => 16,
        HeadKind::Delivery => 17,
    }
}
fn identity(j: &JournalIdentity) -> Result<Vec<u8>, StoreError> {
    r3::canonical_bytes(&json!([j.store, j.scope, j.registration, j.host]), 4096).map_err(core)
}
fn prefix(
    j: &JournalIdentity,
    n: Count,
    s: Digest,
    r: Digest,
) -> Result<TrustedJournalHead, StoreError> {
    let observation = r3::raw_sha256(
        &r3::canonical_bytes(
            &json!(["postgres-primary-journal/1", journal_key(j)?, n, s, r]),
            r3::COMMAND_BYTES,
        )
        .map_err(core)?,
    );
    TrustedJournalHead::from_backend(j.clone(), n, s, r, observation).map_err(core)
}
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Native R3 typed operations await real PG physical admission"
    )
)]
pub(super) enum Operation {
    Locks(JournalIdentity, Vec<Guard>),
    Head(JournalIdentity),
    Lookup(JournalIdentity, wire::Delivery),
    Resolve(ResolveRequest),
    Source(JournalIdentity, Count, wire::FactKind, wire::ProofFullKey),
    Append(Box<ValidatedAdjudicationPlan>),
    #[cfg(test)]
    Primitive(Box<tests::Primitive>),
}
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Native R3 typed results await real PG physical admission"
    )
)]
pub(super) enum Value {
    Unit,
    Head(TrustedJournalHead),
    Saved(Box<Option<SavedOutcome>>),
    Resolved(Box<LockedInputs>),
    Source(Box<VerifiedSource>),
}
pub(super) async fn operation<C: GenericClient + Sync>(
    c: &C,
    held: &mut Locked,
    legacy: &mut super::outcomes::Locked,
    steps: &mut super::outcomes::Steps,
    op: Operation,
) -> Result<Value, StoreError> {
    Ok(match op {
        Operation::Locks(j, g) => {
            locks::acquire(c, held, legacy, &j, &g).await?;
            Value::Unit
        }
        Operation::Head(j) => {
            held.require(&j)?;
            Value::Head(head(c, &j).await?)
        }
        Operation::Lookup(j, k) => {
            held.require(&j)?;
            Value::Saved(Box::new(saved(c, &j, &k).await?))
        }
        Operation::Resolve(q) => {
            held.require(&q.journal)?;
            if held.guards != q.guards {
                return Err(invalid());
            }
            Value::Resolved(Box::new(resolve::locked(c, &q).await?))
        }
        Operation::Source(j, n, k, key) => {
            held.require(&j)?;
            Value::Source(Box::new(source(c, &j, n, &k, &key).await?))
        }
        #[cfg(test)]
        Operation::Primitive(p) => {
            held.require(&p.journal)?;
            tests::primitive(c, &p).await?;
            held.appended = true;
            Value::Unit
        }
        Operation::Append(p) => {
            held.require(p.journal())?;
            persist::plan(c, held, legacy, steps, &p).await?;
            held.appended = true;
            Value::Unit
        }
    })
}
