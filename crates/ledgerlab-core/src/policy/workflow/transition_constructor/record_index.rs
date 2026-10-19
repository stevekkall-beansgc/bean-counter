//! Invocation-local identity locators. Index membership conveys no body trust.
use super::{data::ExactRef, meter, CoreMeter};
use crate::{Error, Result};
use num_traits::ToPrimitive;
use std::cmp::Ordering;

fn number(n: num_bigint::BigInt) -> Result<usize> {
    n.to_usize()
        .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "identity index arithmetic"))
}

#[cfg(test)]
mod tests {
    use super::super::{
        codec,
        data::{OperationKey, RecordInput},
        records::{self, Records},
    };
    use super::*;
    use serde_json::json;
    fn refs() -> Vec<ExactRef> {
        [
            ("b", "xy"),
            ("a", "zz"),
            ("a", "aa"),
            ("é", "a"),
            ("a", "b"),
        ]
        .into_iter()
        .map(|(kind, id)| ExactRef {
            kind: kind.into(),
            id: id.into(),
            digest: "unused".into(),
        })
        .collect()
    }
    #[test]
    fn sorted_index_has_paid_deterministic_search_and_no_source_reordering() {
        let values = refs();
        let borrowed: Vec<_> = values.iter().collect();
        let mut a = CoreMeter::new(65536).unwrap();
        let index = IdentityIndex::build(&mut a, &borrowed).unwrap();
        assert_eq!(index.positions, vec![4, 2, 1, 0, 3]);
        for (i, r) in values.iter().enumerate() {
            assert_eq!(
                index.find(&mut a, &r.kind, &r.id, |i| &values[i]).unwrap(),
                Some(i)
            );
        }
        for (kind, id) in [("", ""), ("a", "c"), ("z", "zzz")] {
            assert_eq!(index.find(&mut a, kind, id, |i| &values[i]).unwrap(), None);
        }
        let mut b = CoreMeter::new(65536).unwrap();
        let again = IdentityIndex::build(&mut b, &borrowed).unwrap();
        for r in &values {
            again.find(&mut b, &r.kind, &r.id, |i| &values[i]).unwrap();
        }
        for (kind, id) in [("", ""), ("a", "c"), ("z", "zzz")] {
            again.find(&mut b, kind, id, |i| &values[i]).unwrap();
        }
        assert_eq!(a.usage(), b.usage());
        assert_eq!(values[0].kind, "b");
    }
    #[test]
    fn duplicate_identity_rejects_equal_and_conflicting_metadata_before_body() {
        for change in 0..3 {
            let mut inputs = vec![
                RecordInput {
                    reference: refs()[0].clone(),
                    canonical_body: b"bad".to_vec(),
                },
                RecordInput {
                    reference: refs()[1].clone(),
                    canonical_body: b"also bad".to_vec(),
                },
            ];
            let mut duplicate = inputs[0].clone();
            if change == 1 {
                duplicate.reference.digest = "other".into();
            }
            if change == 2 {
                duplicate.canonical_body = b"different".to_vec();
            }
            inputs.push(duplicate);
            let mut m = CoreMeter::new(65536).unwrap();
            let err = Records::new(&mut m, &inputs).err().unwrap();
            assert_eq!(err.code, "INTEGRITY_FAILURE");
            assert_eq!(err.detail, "duplicate supplied identity");
            assert_eq!(m.usage().unique_resolved_refs, 0);
        }
        let mut values = refs();
        values[0].id = values[1].id.clone();
        let borrowed: Vec<_> = values.iter().collect();
        assert!(IdentityIndex::build(&mut CoreMeter::new(65536).unwrap(), &borrowed).is_ok());
    }
    #[test]
    fn ordering_accounts_examined_bytes_and_refuses_before_unpaid_operations() {
        let mut m = CoreMeter::new(65536).unwrap();
        assert_eq!(text_order(&mut m, "a", "zz").unwrap(), Ordering::Less);
        assert_eq!(m.usage().scanned_bytes, 0);
        assert_eq!(text_order(&mut m, "ab", "az").unwrap(), Ordering::Less);
        assert_eq!(m.usage().scanned_bytes, 4);
        assert_eq!(text_order(&mut m, "zz", "ab").unwrap(), Ordering::Greater);
        assert_eq!(m.usage().scanned_bytes, 6);
        let values = refs();
        let borrowed: Vec<_> = values.iter().collect();
        let mut full = CoreMeter::new(65536).unwrap();
        IdentityIndex::build(&mut full, &borrowed).unwrap();
        let n = full.usage().semantic_work;
        let mut exact = CoreMeter::new(n).unwrap();
        IdentityIndex::build(&mut exact, &borrowed).unwrap();
        assert_eq!(full.usage(), exact.usage());
        let mut low = CoreMeter::new(n - 1).unwrap();
        assert!(IdentityIndex::build(&mut low, &borrowed).is_err());
        assert!(low.usage().semantic_work < n);
        let index = IdentityIndex::build(&mut CoreMeter::new(65536).unwrap(), &borrowed).unwrap();
        let mut search = CoreMeter::new(65536).unwrap();
        index.find(&mut search, "a", "zz", |i| &values[i]).unwrap();
        let n = search.usage().semantic_work;
        let mut exact = CoreMeter::new(n).unwrap();
        assert_eq!(
            index.find(&mut exact, "a", "zz", |i| &values[i]).unwrap(),
            Some(1)
        );
        assert_eq!(exact.usage(), search.usage());
        assert!(index
            .find(&mut CoreMeter::new(n - 1).unwrap(), "a", "zz", |i| &values
                [i])
            .is_err());
    }
    #[test]
    fn index_is_only_a_locator_and_preserves_consumed_validation() {
        let key = OperationKey {
            logical_store_id: "store".into(),
            scope: ["a".into(), "b".into()],
            source: "source".into(),
            operation_id: "op".into(),
        };
        let mut prepare = CoreMeter::new(65536).unwrap();
        let body = json!({"schema":"ledger-workflow-retained-evidence/1","purpose":"literal"});
        let digest = records::hash(
            &mut prepare,
            "workflow-document",
            &json!([body["schema"], body]),
        )
        .unwrap();
        let reference = ExactRef {
            kind: "workflow-evidence".into(),
            id: format!("wdoc_{}", &digest[7..]),
            digest,
        };
        let input = RecordInput {
            reference: reference.clone(),
            canonical_body: codec::encode(&mut prepare, &body, 4194304).unwrap(),
        };
        let mut m = CoreMeter::new(65536).unwrap();
        let mut r = Records::new(&mut m, &[input.clone()]).unwrap();
        assert!(r.canonical(&mut m, &reference).is_err());
        let mut wrong = reference.clone();
        wrong.digest = format!("sha256:{}", "0".repeat(64));
        assert!(r
            .get(
                &mut m,
                &key,
                &wrong,
                &reference.kind,
                "ledger-workflow-retained-evidence/1"
            )
            .is_err());
        assert_eq!(m.usage().unique_resolved_refs, 0);
        assert_eq!(
            r.get(
                &mut m,
                &key,
                &reference,
                &reference.kind,
                "ledger-workflow-retained-evidence/1"
            )
            .unwrap(),
            body
        );
        assert!(r
            .get(&mut m, &key, &reference, &reference.kind, "wrong-schema")
            .is_err());
        assert_eq!(
            r.get(
                &mut m,
                &key,
                &reference,
                &reference.kind,
                "ledger-workflow-retained-evidence/1"
            )
            .unwrap(),
            body
        );
        assert_eq!(r.consumed(), vec![reference.clone()]);
        let mut bad = input;
        bad.canonical_body.push(b' ');
        let mut m = CoreMeter::new(65536).unwrap();
        let mut r = Records::new(&mut m, &[bad]).unwrap();
        assert!(r
            .get(
                &mut m,
                &key,
                &reference,
                &reference.kind,
                "ledger-workflow-retained-evidence/1"
            )
            .is_err());
    }
}
fn add(m: &mut CoreMeter, a: usize, b: usize) -> Result<usize> {
    number(meter::add(m, &a.into(), &b.into())?)
}
fn less(m: &mut CoreMeter, a: usize, b: usize) -> Result<bool> {
    Ok(meter::cmp(m, &a.into(), &b.into())?.is_lt())
}
fn end(m: &mut CoreMeter, start: usize, width: usize, n: usize) -> Result<usize> {
    let next = add(m, start, width)?;
    Ok(if less(m, next, n)? { next } else { n })
}
// Length-first UTF-8 byte order is internal only, never canonical Ref-set order.
pub(super) fn text_order(m: &mut CoreMeter, a: &str, b: &str) -> Result<Ordering> {
    m.visit("index.string_order")?;
    let lengths = meter::cmp(m, &a.len().into(), &b.len().into())?;
    if !lengths.is_eq() {
        return Ok(lengths);
    }
    for (left, right) in a.bytes().zip(b.bytes()) {
        m.scan_bytes(2)?;
        if left < right {
            return Ok(Ordering::Less);
        }
        if left > right {
            return Ok(Ordering::Greater);
        }
    }
    Ok(Ordering::Equal)
}
fn identity_order(m: &mut CoreMeter, a: &ExactRef, kind: &str, id: &str) -> Result<Ordering> {
    let kinds = text_order(m, &a.kind, kind)?;
    if !kinds.is_eq() {
        return Ok(kinds);
    }
    text_order(m, &a.id, id)
}
fn full_order(m: &mut CoreMeter, a: &ExactRef, b: &ExactRef) -> Result<Ordering> {
    let identity = identity_order(m, a, &b.kind, &b.id)?;
    if !identity.is_eq() {
        return Ok(identity);
    }
    text_order(m, &a.digest, &b.digest)
}
// Only these existing recipes intentionally retain one stable logical record
// ID across immutable revisions. Other semantic identities remain once-only;
// body/schema/domain/lineage checks still execute after the exact locator.
fn revision_identity(m: &mut CoreMeter, r: &ExactRef) -> Result<bool> {
    let invocation = super::shape::same_text(m, &r.kind, "workflow-invocation")?;
    let owner = super::shape::same_text(m, &r.kind, "workflow-cost")?;
    let prefix = if invocation {
        "winv_"
    } else if owner {
        "wcostowner_"
    } else {
        return Ok(false);
    };
    m.visit("index.revision_id_prefix")?;
    if meter::cmp(m, &r.id.len().into(), &prefix.len().into())?.is_lt() {
        return Ok(false);
    }
    super::shape::same_bytes(m, &r.id.as_bytes()[..prefix.len()], prefix.as_bytes())
}

pub(super) struct IdentityIndex {
    positions: Vec<usize>,
    exact_revisions: bool,
}
impl IdentityIndex {
    pub(super) fn build(m: &mut CoreMeter, refs: &[&ExactRef]) -> Result<Self> {
        Self::build_mode(m, refs, false)
    }
    pub(super) fn build_financial(m: &mut CoreMeter, refs: &[&ExactRef]) -> Result<Self> {
        Self::build_mode(m, refs, true)
    }
    fn build_mode(m: &mut CoreMeter, refs: &[&ExactRef], exact_revisions: bool) -> Result<Self> {
        let n = refs.len();
        let mut positions = Vec::with_capacity(n);
        let mut scratch = Vec::with_capacity(n);
        for position in 0..n {
            m.visit("index.initialize_position")?;
            positions.push(position);
            m.visit("index.initialize_scratch")?;
            scratch.push(0);
        }
        let mut width = 1;
        while less(m, width, n)? {
            m.visit("index.merge_pass")?;
            let mut start = 0;
            while less(m, start, n)? {
                m.visit("index.merge_run")?;
                let middle = end(m, start, width, n)?;
                let finish = end(m, middle, width, n)?;
                let (mut left, mut right, mut out) = (start, middle, start);
                while less(m, left, middle)? && less(m, right, finish)? {
                    m.visit("index.merge_candidate")?;
                    m.visit("index.read_left")?;
                    let a = positions[left];
                    m.visit("index.read_right")?;
                    let b = positions[right];
                    let order = if exact_revisions {
                        full_order(m, refs[a], refs[b])?
                    } else {
                        identity_order(m, refs[a], &refs[b].kind, &refs[b].id)?
                    };
                    let selected = if order.is_gt() {
                        right = add(m, right, 1)?;
                        b
                    } else {
                        left = add(m, left, 1)?;
                        a
                    };
                    m.visit("index.write_position")?;
                    scratch[out] = selected;
                    out = add(m, out, 1)?;
                }
                while less(m, left, middle)? {
                    m.visit("index.read_tail")?;
                    let selected = positions[left];
                    m.visit("index.write_position")?;
                    scratch[out] = selected;
                    left = add(m, left, 1)?;
                    out = add(m, out, 1)?;
                }
                while less(m, right, finish)? {
                    m.visit("index.read_tail")?;
                    let selected = positions[right];
                    m.visit("index.write_position")?;
                    scratch[out] = selected;
                    right = add(m, right, 1)?;
                    out = add(m, out, 1)?;
                }
                m.visit("index.next_run")?;
                start = finish;
            }
            m.visit("index.pass_swap")?;
            std::mem::swap(&mut positions, &mut scratch);
            width = number(meter::mul(m, &width.into(), &2usize.into())?)?;
        }
        for pair in positions.windows(2) {
            m.visit("index.adjacent_identity")?;
            let a = refs[pair[0]];
            let b = refs[pair[1]];
            if identity_order(m, a, &b.kind, &b.id)?.is_eq() {
                if exact_revisions
                    && revision_identity(m, a)?
                    && !text_order(m, &a.digest, &b.digest)?.is_eq()
                {
                    continue;
                }
                return Err(Error::new(
                    "INTEGRITY_FAILURE",
                    "duplicate supplied identity",
                ));
            }
        }
        Ok(Self {
            positions,
            exact_revisions,
        })
    }
    pub(super) fn find<'a>(
        &self,
        m: &mut CoreMeter,
        kind: &str,
        id: &str,
        mut reference: impl FnMut(usize) -> &'a ExactRef,
    ) -> Result<Option<usize>> {
        let (mut low, mut high) = (0, self.positions.len());
        while less(m, low, high)? {
            m.visit("index.search_candidate")?;
            let distance = meter::sub(m, &high.into(), &low.into())?;
            let half = meter::div(m, &distance, &2usize.into())?;
            let middle = add(m, low, number(half)?)?;
            m.visit("index.search_position")?;
            let position = self.positions[middle];
            let r = reference(position);
            match identity_order(m, &r, kind, id)? {
                Ordering::Equal => {
                    if self.exact_revisions {
                        if less(m, 0, middle)? {
                            let before = number(meter::sub(m, &middle.into(), &1usize.into())?)?;
                            if identity_order(m, reference(self.positions[before]), kind, id)?
                                .is_eq()
                            {
                                return Err(Error::new(
                                    "INTEGRITY_FAILURE",
                                    "ambiguous supplied identity",
                                ));
                            }
                        }
                        let after = add(m, middle, 1)?;
                        if less(m, after, self.positions.len())?
                            && identity_order(m, reference(self.positions[after]), kind, id)?
                                .is_eq()
                        {
                            return Err(Error::new(
                                "INTEGRITY_FAILURE",
                                "ambiguous supplied identity",
                            ));
                        }
                    }
                    return Ok(Some(position));
                }
                Ordering::Less => low = add(m, middle, 1)?,
                Ordering::Greater => {
                    m.visit("index.search_high")?;
                    high = middle;
                }
            }
        }
        Ok(None)
    }
    pub(super) fn find_exact<'a>(
        &self,
        m: &mut CoreMeter,
        wanted: &ExactRef,
        mut reference: impl FnMut(usize) -> &'a ExactRef,
    ) -> Result<Option<usize>> {
        // The old locator/callback order remains exact for legacy inventories.
        if !self.exact_revisions {
            return self.find(m, &wanted.kind, &wanted.id, reference);
        }
        let (mut low, mut high) = (0, self.positions.len());
        while less(m, low, high)? {
            m.visit("index.search_exact_candidate")?;
            let distance = meter::sub(m, &high.into(), &low.into())?;
            let half = meter::div(m, &distance, &2usize.into())?;
            let middle = add(m, low, number(half)?)?;
            m.visit("index.search_exact_position")?;
            let position = self.positions[middle];
            match full_order(m, reference(position), wanted)? {
                Ordering::Equal => return Ok(Some(position)),
                Ordering::Less => low = add(m, middle, 1)?,
                Ordering::Greater => {
                    m.visit("index.search_exact_high")?;
                    high = middle;
                }
            }
        }
        Ok(None)
    }
}
