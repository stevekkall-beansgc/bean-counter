//! Independent in-memory destination for deterministic delivery/recovery exercises.
//! Keep the same instance across ledger reopen/restore; receipts live independently
//! of ledger transactions. Process-durable fake storage is deferred.
use super::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
#[derive(Clone, Copy, Debug, Default)]
pub enum Mode {
    #[default]
    Normal,
    FailBeforeReceipt,
    LoseResponse,
    Reject,
    UnknownLookup,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub request: Request,
    pub remote_id: String,
}
#[derive(Clone, Debug)]
pub enum Outcome {
    Delivered(Receipt),
    Absent,
    Unknown,
    Rejected,
    Fenced,
}
#[derive(Default)]
struct Data {
    fences: BTreeMap<String, (i64, i64)>,
    receipts: BTreeMap<(String, String), Receipt>,
}
#[derive(Clone, Default)]
pub struct MemoryDestination {
    inner: Arc<Mutex<Data>>,
}
impl MemoryDestination {
    pub fn new() -> Self {
        Self::default()
    }
    pub(crate) fn fence(&self, store: &str, generation: i64, until: i64) -> Result<()> {
        let mut d = self.inner.lock().map_err(|_| Error::Unavailable)?;
        if d.fences.get(store).is_some_and(|(g, _)| *g > generation) {
            return Err(Error::Fenced);
        }
        d.fences.insert(store.into(), (generation, until));
        Ok(())
    }
    pub(crate) fn generation(&self, store: &str) -> i64 {
        self.inner
            .lock()
            .expect("fake lock")
            .fences
            .get(store)
            .map_or(0, |(g, _)| *g)
    }
    /// Simulates receiving a request atomically at the external system. A request
    /// already received before fencing may have executed; reconcile its stable key.
    pub(crate) fn send(&self, a: &Attempt, now: i64, mode: Mode) -> Outcome {
        let mut d = self.inner.lock().expect("fake lock");
        if now >= a.until
            || !d
                .fences
                .get(&a.request.store_id)
                .is_some_and(|(g, until)| *g == a.lease.generation && now < *until)
        {
            return Outcome::Fenced;
        }
        let key = (a.request.store_id.clone(), a.request.key.clone());
        let receipt = if let Some(r) = d.receipts.get(&key) {
            if r.request != a.request {
                return Outcome::Rejected;
            }
            r.clone()
        } else {
            // An existing receipt can never be reported authoritatively absent,
            // including when the test requests a pre-receipt failure.
            if matches!(mode, Mode::FailBeforeReceipt) {
                return Outcome::Absent;
            }
            if matches!(mode, Mode::Reject) {
                return Outcome::Rejected;
            }
            let receipt = Receipt {
                request: a.request.clone(),
                remote_id: format!("fake:{}", a.request.key),
            };
            d.receipts.insert(key, receipt.clone());
            receipt
        };
        if matches!(mode, Mode::LoseResponse) {
            Outcome::Unknown
        } else {
            Outcome::Delivered(receipt)
        }
    }
    pub fn receipts(&self, store: &str) -> Vec<Receipt> {
        self.inner
            .lock()
            .expect("fake lock")
            .receipts
            .iter()
            .filter(|((s, _), _)| s == store)
            .map(|(_, r)| r.clone())
            .collect()
    }
    pub(crate) fn lookup(&self, store: &str, key: &str, mode: Mode) -> Outcome {
        if matches!(mode, Mode::UnknownLookup) {
            return Outcome::Unknown;
        }
        self.inner
            .lock()
            .expect("fake lock")
            .receipts
            .get(&(store.into(), key.into()))
            .map_or(Outcome::Absent, |r| Outcome::Delivered(r.clone()))
    }
    pub(crate) fn keys_page(&self, store: &str, after: &str) -> Vec<String> {
        self.inner
            .lock()
            .expect("fake lock")
            .receipts
            .range((
                std::ops::Bound::Excluded((store.into(), after.into())),
                std::ops::Bound::Unbounded,
            ))
            .take_while(|((s, _), _)| s == store)
            .take(PAGE_SIZE)
            .map(|((_, key), _)| key.clone())
            .collect()
    }
    pub(crate) fn inventory_digest(&self, store: &str, mode: Mode) -> Result<Option<String>> {
        if matches!(mode, Mode::UnknownLookup) {
            return Ok(None);
        }
        let data = self.inner.lock().map_err(|_| Error::Unavailable)?;
        let mut hash = digest(&json!(["fake-inventory-v2", store]))?;
        for ((s, _), r) in data.receipts.range((store.to_owned(), String::new())..) {
            if s != store {
                break;
            }
            hash = digest(&json!([hash, r.request.value(), r.remote_id]))?;
        }
        Ok(Some(hash))
    }
    #[cfg(test)]
    pub(crate) fn insert_receipt(&self, request: Request) {
        self.inner.lock().unwrap().receipts.insert(
            (request.store_id.clone(), request.key.clone()),
            Receipt {
                request,
                remote_id: "test-remote".into(),
            },
        );
    }
}
