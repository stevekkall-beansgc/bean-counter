//! Invocation-local identity locators. Index membership conveys no body trust.
use super::{CoreMeter, data::ExactRef, meter};
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
        assert!(
            index
                .find(&mut CoreMeter::new(n - 1).unwrap(), "a", "zz", |i| &values
                    [i])
                .is_err()
        );
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
        assert!(
            r.get(
                &mut m,
                &key,
                &wrong,
                &reference.kind,
                "ledger-workflow-retained-evidence/1"
            )
            .is_err()
        );
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
        assert!(
            r.get(&mut m, &key, &reference, &reference.kind, "wrong-schema")
                .is_err()
        );
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
        assert!(
            r.get(
                &mut m,
                &key,
                &reference,
                &reference.kind,
                "ledger-workflow-retained-evidence/1"
            )
            .is_err()
        );
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

pub(super) struct IdentityIndex {
    positions: Vec<usize>,
}
impl IdentityIndex {
    pub(super) fn build(m: &mut CoreMeter, refs: &[&ExactRef]) -> Result<Self> {
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
                    let order = identity_order(m, refs[a], &refs[b].kind, &refs[b].id)?;
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
                return Err(Error::new(
                    "INTEGRITY_FAILURE",
                    "duplicate supplied identity",
                ));
            }
        }
        Ok(Self { positions })
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
                Ordering::Equal => return Ok(Some(position)),
                Ordering::Less => low = add(m, middle, 1)?,
                Ordering::Greater => {
                    m.visit("index.search_high")?;
                    high = middle;
                }
            }
        }
        Ok(None)
    }
}
