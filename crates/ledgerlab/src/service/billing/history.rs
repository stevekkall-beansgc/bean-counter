use super::*;
use crate::store::sqlite::BillingSnapshot;
pub(super) struct History {
    pub records: Records,
    pub base: b::Base,
    pub decisions: Vec<c::outcomes::Decision>,
}
pub(super) struct Audit {
    pub targets: BTreeMap<String, History>,
    pub receipts: BTreeMap<i64, Value>,
    pub rows: BTreeMap<i64, Vec<Value>>,
}
pub(super) fn load(s: &Setup, snapshot: &BillingSnapshot) -> Result<Audit> {
    let mut audit = Audit {
        targets: BTreeMap::new(),
        receipts: BTreeMap::new(),
        rows: BTreeMap::new(),
    };
    let mut identities = BTreeSet::new();
    let mut semantics = BTreeSet::new();
    for (i, e) in snapshot.entries.iter().enumerate() {
        b::check(
            e.ordinal == i as i64 + 1
                && identities.insert((&e.source, &e.external_id))
                && semantics.insert((&e.source, &e.semantic_key)),
        )?;
        let v = core(canonical::parse_bounded(&e.bundle, 8 * 1024 * 1024))?;
        b::check(bytes(&v)? == e.bundle)?;
        let rows = array(&v)?.clone();
        if rows.iter().any(|r| r["kind"] == "base-acceptance") {
            let records = decode_entry(s, e)?;
            permissions::verify_grants(&records, snapshot)?;
            let receipt = records.one("base-acceptance")?.clone();
            b::check(rows.len() == array(&receipt["body"]["members"])?.len() + 1)?;
            let base = b::decode_base(&records, &reference(&receipt))?;
            let target = text(&receipt["body"]["target"])?.to_owned();
            b::check(
                audit
                    .targets
                    .insert(
                        target,
                        History {
                            records,
                            base,
                            decisions: vec![],
                        },
                    )
                    .is_none(),
            )?;
            audit.receipts.insert(e.ordinal, receipt);
        } else {
            let group = Records::new(
                &rows.iter().map(bytes).collect::<Result<Vec<_>>>()?,
                json!(s.scope),
            )?;
            let event = group.one("event")?;
            let target = text(&event["body"]["data"]["target"])?;
            let history = audit.targets.get_mut(target).ok_or_else(b::integrity)?;
            adjustment::verify_entry(s, e, &group)?;
            for r in &rows {
                history.records.insert(r.clone())?;
            }
            permissions::verify_grants(&history.records, snapshot)?;
            let groups = super::super::retained::decision_groups(&history.records, &history.base)?;
            history.decisions =
                super::super::retained::economic::replay(&history.records, &history.base, &groups)?;
            audit
                .receipts
                .insert(e.ordinal, group.one("receipt")?.clone());
        }
        audit.rows.insert(e.ordinal, rows);
    }
    for a in &snapshot.aliases {
        b::check(identities.insert((&a.source, &a.external_id)))?;
        let e = snapshot
            .entries
            .iter()
            .find(|e| e.ordinal == a.ordinal)
            .ok_or_else(b::integrity)?;
        b::check(a.source == e.source)?;
        if audit.receipts[&e.ordinal]["kind"] == "base-acceptance" {
            let event = domain::normalize(&a.ingress, s.scope.clone(), &s.source)
                .map_err(|_| b::integrity())?
                .resolve(None)
                .map_err(|_| b::integrity())?;
            b::check(
                event.candidate().external_id() == a.external_id
                    && event.candidate().ingress_bytes().as_slice() == a.ingress
                    && bytes(&json!(["base", event.candidate().operation_id()]))? == e.semantic_key
                    && bytes(&event.completion_facts(&[]).map_err(|_| b::integrity())?)? == e.facts,
            )?;
        } else {
            adjustment::verify_alias(a, e)?;
        }
    }
    Ok(audit)
}
