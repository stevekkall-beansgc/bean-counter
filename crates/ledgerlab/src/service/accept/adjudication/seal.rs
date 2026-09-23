//! Bounded scan under the SAME live gateway transaction. The continuation never
//! becomes a caller DTO; cancellation drops it without a business-state write.
use super::*;
use crate::{service::store_error, ServiceError};
use ledgerlab_core::adjudication::{
    self as r3,
    runtime::{points as p, seal::SealFold},
};
pub(super) struct VerifiedSealScan {
    transaction: Digest,
    incarnation: Digest,
    epoch: Count,
    prefix: TrustedJournalHead,
    seal: wire::Seal,
}
impl VerifiedSealScan {
    pub(super) fn check(&self, cap: &CommitCapability) -> ledgerlab_core::Result<()> {
        if self.transaction == *cap.transaction()
            && self.incarnation == *cap.storage_incarnation()
            && self.epoch == cap.epoch()
            && self.prefix.journal() == cap.journal()
            && self.prefix.ordinal() == cap.recovered_through().ordinal()
            && self.prefix.segment() == cap.recovered_through().segment()
            && self.prefix.root() == cap.recovered_through().root()
        {
            Ok(())
        } else {
            Err(ledgerlab_core::Error {
                code: "SCAN_LEASE_CHANGED",
                detail: "live transaction binding".into(),
            })
        }
    }
    pub(super) fn value(&self) -> &wire::Seal {
        &self.seal
    }
}
fn core<T>(x: ledgerlab_core::Result<T>) -> Result<T, ServiceError> {
    x.map_err(|e| ServiceError::Rejection(e.code.into()))
}
async fn point<T: AdjudicationTx>(
    tx: &mut T,
    cap: &CommitCapability,
    key: &wire::Delivery,
    guards: &[Guard],
    kind: HeadKind,
    full_key: Vec<u8>,
) -> Result<p::State, ServiceError> {
    let requested = HeadKey {
        journal: cap.journal().clone(),
        kind,
        full_key,
    };
    let resolution = tx
        .resolve_adjudication(&ResolveRequest {
            journal: cap.journal().clone(),
            key: key.clone(),
            guards: guards.to_vec(),
            objects: vec![],
            heads: vec![requested.clone()],
        })
        .await
        .map_err(store_error)?;
    let Resolution::Complete(inputs) = resolution else {
        return Err(ServiceError::IntegrityFailure);
    };
    if inputs.journal != *cap.journal()
        || inputs.prefix.ordinal() != cap.recovered_through().ordinal()
        || inputs.prefix.segment() != cap.recovered_through().segment()
        || inputs.prefix.root() != cap.recovered_through().root()
        || inputs.heads.len() != 1
        || inputs.heads[0].key != requested
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let raw = inputs.heads[0]
        .value
        .as_deref()
        .ok_or_else(|| ServiceError::Rejection("SEAL_SCAN_HOLE".into()))?;
    if inputs.heads[0].revision.is_none() {
        return Err(ServiceError::IntegrityFailure);
    }
    core(super::prepare::parsed_state(raw))
}
pub(super) async fn scan<T: AdjudicationTx>(
    tx: &mut T,
    cap: &CommitCapability,
    key: &wire::Delivery,
    guards: &[Guard],
    round: Count,
    gateway: Id,
    cutoff: Count,
    high: Count,
) -> Result<VerifiedSealScan, ServiceError> {
    if gateway != cap.journal().host {
        return Err(ServiceError::IntegrityFailure);
    }
    let mut fold = SealFold::new(gateway.clone(), round, cutoff, high);
    while let Some(position) = fold.next_allocation() {
        let p = core(p::Point::position(
            p::PointKind::Allocation,
            &gateway,
            position,
        ))?;
        let p::State::Position(token) =
            point(tx, cap, key, guards, HeadKind::Allocation, p.key).await?
        else {
            return Err(ServiceError::IntegrityFailure);
        };
        let k = core(p::Point::id(
            p::PointKind::Token,
            *b"TOKEN___",
            token.as_str(),
        ))?;
        let p::State::Token(t) = point(tx, cap, key, guards, HeadKind::Token, k.key).await? else {
            return Err(ServiceError::IntegrityFailure);
        };
        core(fold.disposition(&t))?;
    }
    while let Some(position) = fold.next_receipt() {
        let p = core(p::Point::position(
            p::PointKind::Receipt,
            &gateway,
            position,
        ))?;
        let p::State::Position(token) =
            point(tx, cap, key, guards, HeadKind::Receipt, p.key).await?
        else {
            return Err(ServiceError::IntegrityFailure);
        };
        let k = core(p::Point::id(
            p::PointKind::Token,
            *b"TOKEN___",
            token.as_str(),
        ))?;
        let p::State::Token(t) = point(tx, cap, key, guards, HeadKind::Token, k.key).await? else {
            return Err(ServiceError::IntegrityFailure);
        };
        if t.token.id != token || t.status != p::TokenStatus::NewCase {
            return Err(ServiceError::IntegrityFailure);
        }
        let r = t.receipt.as_ref().ok_or(ServiceError::IntegrityFailure)?;
        core(fold.receipt(r))?;
    }
    let value = VerifiedSealScan {
        transaction: cap.transaction().clone(),
        incarnation: cap.storage_incarnation().clone(),
        epoch: cap.epoch(),
        prefix: cap.recovered_through().clone(),
        seal: core(fold.finish())?,
    };
    core(value.check(cap))?;
    let _ = r3::PAGE_BYTES;
    Ok(value)
}
