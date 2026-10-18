//! Exact shared assertion identity. A projection hit is DATA, requiring the
//! completed original and immutable canonical body before reuse by the child.
use super::effects::{same_ref, same_text};
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::{
    EconomicCandidate, ExactRef, PermanentClaimKey,
};

const LENGTHS: &str = "SELECT octet_length(record_kind),octet_length(record_id),octet_length(record_digest) FROM workflow_assertion_claims WHERE logical_store_id=? AND tenant=? AND environment=? AND verified_source=? AND attestation_id=?";
const VALUES: &str = "SELECT record_kind,record_id,record_digest FROM workflow_assertion_claims WHERE logical_store_id=? AND tenant=? AND environment=? AND verified_source=? AND attestation_id=?";

fn parts<'a>(
    key: &'a PermanentClaimKey,
    s: &mut OriginalAuditSession,
) -> Result<[&'a str; 5], FinancialReadError> {
    s.semantic_work().visit(ValidationVisit::Constraint)?;
    let PermanentClaimKey::Assertion {
        logical_store_id,
        scope,
        verified_source,
        attestation_id,
    } = key
    else {
        return Err(StoreError::Integrity("non-assertion permanent key").into());
    };
    let values = [
        logical_store_id.as_str(),
        scope[0].as_str(),
        scope[1].as_str(),
        verified_source.as_str(),
        attestation_id.as_str(),
    ];
    for (value, maximum) in values.into_iter().zip([128usize, 128, 128, 256, 128]) {
        s.semantic_work().visit(ValidationVisit::Element)?;
        heads::locator_text(value, maximum, s)?;
    }
    Ok(values)
}

impl SqliteTx {
    pub(crate) async fn workflow_assertion_claim(
        &mut self,
        key: &PermanentClaimKey,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<AppClaimLocator>, FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned assertion lookup").into());
        }
        self.failed = true;
        let result = timeout_at(self.deadline, async {
            let values = parts(key, s)?;
            let bound = values.map(str::as_bytes);
            self.read_financial_claim_projection(LENGTHS, VALUES, &values, &bound, s)
                .await
        })
        .await
        .map_err(|_| FinancialReadError::Store(StoreError::Deadline))?;
        if result.is_ok() {
            s.semantic_work().visit(ValidationVisit::Constraint)?;
            s.check_cancelled()?;
            if self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
                || self.financial_io.is_some()
            {
                return Err(StoreError::Integrity("assertion lookup retained worker").into());
            }
            self.failed = false;
        }
        result
    }

    pub(super) async fn insert_assertion_claim(
        &mut self,
        candidate: &EconomicCandidate,
        key: &PermanentClaimKey,
        r: &ExactRef,
        s: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        s.semantic_work().visit(ValidationVisit::Constraint)?;
        if !self.failed {
            return Err(StoreError::Integrity("assertion effect outside append").into());
        }
        let values = parts(key, s)?;
        let captured = candidate.key();
        if !same_text(values[0], &captured.logical_store_id, s)?
            || !same_text(values[1], &captured.scope[0], s)?
            || !same_text(values[2], &captured.scope[1], s)?
            || !same_text(&r.kind, "workflow-fact-attestation", s)?
        {
            return Err(StoreError::Integrity("assertion effect scope/kind").into());
        }
        // The same exact pair may be observed again, but it cannot acquire a
        // second body. Read the old Ref under this writer; never ignore a key
        // conflict or replace a previously introduced assertion.
        let bound = values.map(str::as_bytes);
        let old = self
            .read_financial_claim_projection(LENGTHS, VALUES, &values, &bound, s)
            .await?;
        s.semantic_work().visit(ValidationVisit::Constraint)?;
        if let Some(old) = old {
            for (actual, expected) in [
                (&old.kind, &r.kind),
                (&old.id, &r.id),
                (&old.digest, &r.digest),
            ] {
                let actual = effects::head_text(actual, s)?;
                if !same_text(actual, expected, s)? {
                    return Err(StoreError::Integrity("assertion permanent key conflict").into());
                }
            }
            return Ok(());
        }
        // A missing projection cannot be repaired by a reused result: its
        // original introduction must have installed this permanent pair.
        let mut introduced = false;
        for record in candidate.introduced() {
            s.semantic_work().visit(ValidationVisit::Element)?;
            if same_ref(record.reference(), r, s)? {
                introduced = true;
                break;
            }
        }
        s.semantic_work().visit(ValidationVisit::Constraint)?;
        if !introduced {
            return Err(
                StoreError::Integrity("reused assertion missing permanent projection").into(),
            );
        }
        let parameters = [
            values[0].as_bytes(),
            values[1].as_bytes(),
            values[2].as_bytes(),
            values[3].as_bytes(),
            values[4].as_bytes(),
            r.kind.as_bytes(),
            r.id.as_bytes(),
            r.digest.as_bytes(),
        ];
        let payload = io::binding_payload_sets(&parameters, &[&parameters, &parameters], 2, 0, s)?;
        self.financial_reserve_io(s, payload)?;
        s.semantic_work().write_bytes(std::mem::size_of::<u64>())?;
        s.semantic_work().visit(ValidationVisit::RefLookup)?;
        let affected = sqlx::query("INSERT INTO workflow_assertion_claims VALUES(?,?,?,?,?,?,?,?)")
            .bind(values[0])
            .bind(values[1])
            .bind(values[2])
            .bind(values[3])
            .bind(values[4])
            .bind(&r.kind)
            .bind(&r.id)
            .bind(&r.digest)
            .execute(self.conn())
            .await?
            .rows_affected();
        self.financial_finish_io(s).await?;
        if !resource_compare(s, i64::try_from(affected).map_err(|_| invalid())?, 1)?.is_eq() {
            return Err(invalid());
        }
        Ok(())
    }
}
