//! Constant-state ordered fold over indexed immutable local disposition/receipt
//! projections. Source acquisition and the live transaction lease belong to the
//! coordinator; caller-provided digests never enter this fold.
use super::points::{TokenState, TokenStatus};
use crate::adjudication::{
    canonical_bytes, commands as w,
    types::{Count, Digest, Id},
    Validate,
};
use crate::{Error, Result};
use sha2::{Digest as _, Sha256};
pub struct SealFold {
    gateway: Id,
    round: Count,
    cutoff: Count,
    high: Count,
    allocation: Count,
    receipt: Count,
    dispositions: Sha256,
    receipts: Sha256,
}
fn require(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::new(
            "SEAL_SCAN_HOLE",
            "ordered local exact membership",
        ))
    }
}
impl SealFold {
    pub fn new(gateway: Id, round: Count, cutoff: Count, high: Count) -> Self {
        let mut d = Sha256::new();
        d.update(b"ledgerlab/central-r3/result/1\0[");
        let mut r = Sha256::new();
        r.update(b"ledgerlab/central-r3/receipt/1\0[");
        Self {
            gateway,
            round,
            cutoff,
            high,
            allocation: Count::ZERO,
            receipt: Count::ZERO,
            dispositions: d,
            receipts: r,
        }
    }
    pub fn disposition(&mut self, t: &TokenState) -> Result<usize> {
        let n = self.allocation.checked_add(Count::new(1)?)?;
        require(n <= self.cutoff && t.token.gateway == self.gateway && t.token.allocation == n)?;
        let state = match t.status {
            TokenStatus::NewCase => "NEW_CASE",
            TokenStatus::Alias => "ALIAS",
            TokenStatus::ReturnedUnused => "RETURNED_UNUSED",
            _ => return Err(Error::new("UNRESOLVED_TOKEN", "no local disposition")),
        };
        let raw = canonical_bytes(
            &serde_json::json!([t.token.allocation, t.token.id, state]),
            16384,
        )?;
        if self.allocation != Count::ZERO {
            self.dispositions.update(b",");
        }
        self.dispositions.update(&raw);
        self.allocation = n;
        Ok(raw.len() + usize::from(n.value() > 1))
    }
    pub fn receipt(&mut self, r: &w::Receipt) -> Result<usize> {
        r.validate()?;
        let n = self.receipt.checked_add(Count::new(1)?)?;
        require(n <= self.high && r.position == n && r.gateway == self.gateway)?;
        let raw = canonical_bytes(&serde_json::json!([r.position, r]), 16384)?;
        if self.receipt != Count::ZERO {
            self.receipts.update(b",");
        }
        self.receipts.update(&raw);
        self.receipt = n;
        Ok(raw.len() + usize::from(n.value() > 1))
    }
    pub fn finish(mut self) -> Result<w::Seal> {
        require(self.allocation == self.cutoff && self.receipt == self.high)?;
        self.dispositions.update(b"]");
        self.receipts.update(b"]");
        let disposition_root =
            Digest::parse(&crate::canonical::hex(&self.dispositions.finalize()))?;
        let receipt_root = Digest::parse(&crate::canonical::hex(&self.receipts.finalize()))?;
        Ok(w::Seal {
            round: self.round,
            gateway: self.gateway,
            cutoff: self.cutoff,
            receipt_high: self.high,
            disposition_root,
            receipt_root,
        })
    }
    pub fn next_allocation(&self) -> Option<Count> {
        (self.allocation < self.cutoff)
            .then(|| Count::new(self.allocation.value() + 1).expect("bounded next"))
    }
    pub fn next_receipt(&self) -> Option<Count> {
        (self.receipt < self.high)
            .then(|| Count::new(self.receipt.value() + 1).expect("bounded next"))
    }
}
#[test]
fn empty_fold_is_domain_separated_and_not_a_raw_digest() {
    let seal = SealFold::new(
        Id::parse("g").unwrap(),
        Count::new(1).unwrap(),
        Count::ZERO,
        Count::ZERO,
    )
    .finish()
    .unwrap();
    assert_eq!(
        seal.disposition_root,
        super::hash("result", &Vec::<String>::new()).unwrap()
    );
    assert_eq!(
        seal.receipt_root,
        super::hash("receipt", &Vec::<String>::new()).unwrap()
    );
    assert_ne!(seal.receipt_root, crate::adjudication::raw_sha256(b"[]"));
}
