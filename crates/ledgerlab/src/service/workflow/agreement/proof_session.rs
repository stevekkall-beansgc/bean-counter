//! Completed immutable proofs for one lexical held-transaction invocation.
//! This is neither an authority capability nor a cache of mutable eligibility.
use super::super::request::OperationKey;
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;

#[derive(Clone)]
struct Complete {
    binding: String,
    original: bool,
}
struct State {
    transaction: usize,
    failure: crate::store::sqlite::WorkflowProofFailureHandle,
    store: String,
    scope: [String; 2],
    invalid: bool,
    active: BTreeSet<Vec<u8>>,
    complete: BTreeMap<Vec<u8>, Complete>,
    local_parent: Option<Vec<u8>>,
}
impl State {
    fn invalidate(&mut self) {
        self.invalid = true;
        self.active.clear();
        self.complete.clear();
        self.local_parent = None;
        self.failure
            .store(true, std::sync::atomic::Ordering::Release);
    }
}
tokio::task_local! { static SESSION: std::cell::RefCell<State>; }

fn bound(state: &State, tx: &mut SqliteTx, key: &OperationKey) -> bool {
    !state.invalid
        && state.transaction == tx.workflow_proof_failure_handle().identity()
        && state.store == key.logical_store_id
        && state.scope == key.scope
}
pub(in crate::service::workflow) fn present(tx: &mut SqliteTx, key: &OperationKey) -> Result<bool> {
    let failure = tx.workflow_proof_failure_handle();
    if failure.load(std::sync::atomic::Ordering::Acquire) {
        let _ = SESSION.try_with(|state| state.borrow_mut().invalidate());
        return Err(ServiceError::IntegrityFailure);
    }
    match SESSION.try_with(|state| {
        let mut state = state.borrow_mut();
        if bound(&state, tx, key) {
            true
        } else {
            state.invalidate();
            failure.store(true, std::sync::atomic::Ordering::Release);
            false
        }
    }) {
        Ok(true) => Ok(true),
        Ok(false) => Err(ServiceError::IntegrityFailure),
        Err(_) => Ok(false),
    }
}

/// The per-transaction mechanical poison handle is a private instance token;
/// it is not caller-supplied authority. No proof state survives this future.
pub(in crate::service::workflow) fn with_proofs<'a, T: Send + 'a>(
    tx: &'a mut SqliteTx,
    key: &OperationKey,
    run: impl FnOnce(&'a mut SqliteTx) -> Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>
    + Send
    + 'a,
) -> Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>> {
    match present(tx, key) {
        Ok(true) => return run(tx),
        Err(e) => return Box::pin(async move { Err(e) }),
        Ok(false) => {}
    }
    let failure = tx.workflow_proof_failure_handle();
    let state = State {
        transaction: failure.identity(),
        failure: failure.clone(),
        store: key.logical_store_id.clone(),
        scope: key.scope.clone(),
        invalid: false,
        active: BTreeSet::new(),
        complete: BTreeMap::new(),
        local_parent: None,
    };
    let mut exit = ScopeExit {
        failure,
        complete: false,
    };
    Box::pin(SESSION.scope(std::cell::RefCell::new(state), async move {
        let result = run(tx).await;
        let clean = SESSION.with(|state| {
            let state = state.borrow();
            !state.invalid && state.active.is_empty() && state.local_parent.is_none()
        });
        match result {
            // Poison and discard partial state without changing the original
            // retryable/domain error. A caught failure followed by Ok is denied.
            Err(error) => Err(error),
            Ok(value) if clean => {
                exit.mark_result(true);
                Ok(value)
            }
            Ok(_) => Err(ServiceError::IntegrityFailure),
        }
    }))
}

struct ScopeExit {
    failure: crate::store::sqlite::WorkflowProofFailureHandle,
    complete: bool,
}
impl ScopeExit {
    fn mark_result(&mut self, success: bool) {
        self.complete = success;
    }
}
impl Drop for ScopeExit {
    fn drop(&mut self) {
        if !self.complete {
            self.failure
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
}

/// Appending changes the snapshot. Prove the new certificate in a new epoch;
/// no positive or absent-origin result is carried through that boundary.
pub(in crate::service::workflow) fn after_append(
    tx: &mut SqliteTx,
    key: &OperationKey,
) -> Result<()> {
    SESSION
        .try_with(|state| {
            let mut state = state.borrow_mut();
            if !bound(&state, tx, key) || !state.active.is_empty() || state.local_parent.is_some() {
                state.invalidate();
                tx.workflow_proof_failure_handle()
                    .store(true, std::sync::atomic::Ordering::Release);
                return Err(ServiceError::IntegrityFailure);
            }
            state.complete.clear();
            Ok(())
        })
        .map_err(|_| ServiceError::IntegrityFailure)?
}

pub(super) enum Entry {
    Completed,
    Started(Guard),
}

/// Local semantic validation can consume completed historical proofs only.
/// This view cannot be constructed from an index row or an active entry.
pub(super) struct CompletedOrigin {
    _completed: (),
}
pub(super) fn completed(
    tx: &mut SqliteTx,
    key: &OperationKey,
    identity: &[u8],
    binding: &str,
    original: bool,
) -> Result<CompletedOrigin> {
    SESSION
        .try_with(|cell| {
            let mut state = cell.borrow_mut();
            let valid = bound(&state, tx, key)
                && state
                    .complete
                    .get(identity)
                    .is_some_and(|proof| proof.binding == binding && (!original || proof.original));
            if !valid {
                state.invalidate();
                tx.workflow_proof_failure_handle()
                    .store(true, std::sync::atomic::Ordering::Release);
                return Err(ServiceError::IntegrityFailure);
            }
            Ok(CompletedOrigin { _completed: () })
        })
        .map_err(|_| ServiceError::IntegrityFailure)?
}
pub(super) fn local_phase(tx: &mut SqliteTx, key: &OperationKey) -> Result<bool> {
    if !present(tx, key)? {
        return Ok(false);
    }
    SESSION.with(|cell| Ok(cell.borrow().local_parent.is_some()))
}
pub(super) struct LocalPhase {
    parent: Vec<u8>,
    finished: bool,
}
impl LocalPhase {
    pub(super) fn finish(mut self) -> Result<()> {
        SESSION.with(|cell| {
            let mut state = cell.borrow_mut();
            if state.invalid || state.local_parent.as_deref() != Some(self.parent.as_slice()) {
                state.invalidate();
                return Err(ServiceError::IntegrityFailure);
            }
            state.local_parent = None;
            Ok(())
        })?;
        self.finished = true;
        Ok(())
    }
}
impl Drop for LocalPhase {
    fn drop(&mut self) {
        if !self.finished {
            let _ = SESSION.try_with(|cell| cell.borrow_mut().invalidate());
        }
    }
}
pub(super) fn begin_local(
    tx: &mut SqliteTx,
    key: &OperationKey,
    parent: &[u8],
) -> Result<LocalPhase> {
    SESSION
        .try_with(|cell| {
            let mut state = cell.borrow_mut();
            if !bound(&state, tx, key)
                || state.local_parent.is_some()
                || !state.active.contains(parent)
            {
                state.invalidate();
                tx.workflow_proof_failure_handle()
                    .store(true, std::sync::atomic::Ordering::Release);
                return Err(ServiceError::IntegrityFailure);
            }
            state.local_parent = Some(parent.to_vec());
            Ok(LocalPhase {
                parent: parent.to_vec(),
                finished: false,
            })
        })
        .map_err(|_| ServiceError::IntegrityFailure)?
}
/// Dropping an unfinished proof invalidates the entire scope. An in-progress
/// entry is never accepted as a way to terminate a recursive dependency.
pub(super) struct Guard {
    identity: Vec<u8>,
    binding: String,
    finished: bool,
}
impl Guard {
    pub(super) fn finish(mut self, original: bool) -> Result<()> {
        SESSION
            .try_with(|state| {
                let mut state = state.borrow_mut();
                if state.invalid
                    || state.local_parent.is_some()
                    || !state.active.remove(&self.identity)
                {
                    return Err(ServiceError::IntegrityFailure);
                }
                state.complete.insert(
                    self.identity.clone(),
                    Complete {
                        binding: self.binding.clone(),
                        original,
                    },
                );
                Ok(())
            })
            .map_err(|_| ServiceError::IntegrityFailure)??;
        self.finished = true;
        Ok(())
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if !self.finished {
            let _ = SESSION.try_with(|state| {
                let mut state = state.borrow_mut();
                state.invalidate();
            });
        }
    }
}
pub(super) fn enter(
    tx: &mut SqliteTx,
    key: &OperationKey,
    identity: &[u8],
    binding: String,
    must_be_original: bool,
) -> Result<Entry> {
    SESSION
        .try_with(|state| {
            let mut state = state.borrow_mut();
            if !bound(&state, tx, key) {
                state.invalidate();
                tx.workflow_proof_failure_handle()
                    .store(true, std::sync::atomic::Ordering::Release);
                return Err(ServiceError::IntegrityFailure);
            }
            if state.local_parent.is_some() {
                // A local continuation cannot schedule a new certificate, even
                // if a future caller bypasses the completed-only entry helper.
                state.invalidate();
                return Err(ServiceError::IntegrityFailure);
            }
            if let Some(complete) = state.complete.get(identity) {
                if complete.binding != binding || (must_be_original && !complete.original) {
                    state.invalidate();
                    return Err(ServiceError::IntegrityFailure);
                }
                return Ok(Entry::Completed);
            }
            // The installed Profile B operation bound counts both publication
            // and agreement identities; duplicates do not consume this set.
            if state.active.contains(identity)
                || tx.workflow_snapshot.as_ref().and_then(|s|s.settings.as_ref()).map(|r|r.resolved_records).unwrap_or(Some(100_000)).is_some_and(|maximum|state.active.len().checked_add(state.complete.len()).is_none_or(|n|n>=maximum))
            {
                state.invalidate();
                return Err(ServiceError::IntegrityFailure);
            }
            state.active.insert(identity.to_vec());
            Ok(Entry::Started(Guard {
                identity: identity.to_vec(),
                binding,
                finished: false,
            }))
        })
        .map_err(|_| ServiceError::IntegrityFailure)?
}

/// Internal memo binding, not a new normative identity. Check every row copy
/// and member on a hit; retain only this compact digest and originalness bit.
pub(super) fn journal_binding(o: &JournalOperation) -> Result<String> {
    let mut members = o.members.clone();
    records::sort_refs(&mut members)?;
    Ok(records::raw_hash(&records::bytes(&json!([
        o.identity,
        o.kind,
        o.operation_ref,
        records::raw_hash(&o.operation_bytes),
        o.request_hash,
        o.manifest_ref,
        records::raw_hash(&o.manifest_bytes),
        o.receipt_ref,
        records::raw_hash(&o.receipt_bytes),
        o.sequence,
        o.accepted_at_us,
        members
    ]))?))
}
pub(super) fn publication_binding(
    o: &crate::store::sqlite::workflow::StoredOperation,
) -> Result<String> {
    let mut members = o.members.clone();
    records::sort_refs(&mut members)?;
    Ok(records::raw_hash(&records::bytes(&json!([
        o.identity,
        "policy.publish",
        o.operation_ref,
        records::raw_hash(&o.operation_bytes),
        o.request_hash,
        o.manifest_ref,
        records::raw_hash(&o.manifest_bytes),
        o.receipt_ref,
        records::raw_hash(&o.receipt_bytes),
        o.sequence,
        o.accepted_at_us,
        o.publication_id,
        o.plan_id,
        members
    ]))?))
}
pub(super) fn journal_response(o: &JournalOperation) -> Result<Value> {
    let op = verify::checked(&o.operation_bytes, 8388608)?;
    let receipt = verify::checked(&o.receipt_bytes, 1048576)?;
    Ok(
        json!({"schema":"ledger-operation-status/1","status":"committed",
        "receipt":receipt,"request_evidence":op["request_evidence"]}),
    )
}

#[cfg(test)]
mod tests;
