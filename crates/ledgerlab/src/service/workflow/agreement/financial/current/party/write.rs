//! Borrowed physical operands retained behind actual party preparation.
//! This is DATA; no type here establishes completed resources or admission.
use super::super::super::{evidence::RequestEvidence, ingress::IncomingRequest, wire};
use super::produce::PendingPartyOperation;
use super::*;
use crate::store::sqlite::workflow::financial::{FinancialDocument, PartyAppend, PartyHeadEffect};

pub(in super::super::super) struct PartyWriteData<'a> {
    pending: &'a PendingPartyOperation,
    incoming: &'a IncomingRequest,
    evidence: &'a RequestEvidence,
    journal: &'a wire::PartyJournalData<'a>,
    documents: [FinancialDocument<'a>; 3],
    result: AuditedValue,
}
impl<'a> PartyWriteData<'a> {
    pub(in super::super::super) fn new(
        pending: &'a PendingPartyOperation,
        incoming: &'a IncomingRequest,
        evidence: &'a RequestEvidence,
        journal: &'a wire::PartyJournalData<'a>,
        session: &mut OriginalAuditSession,
    ) -> Result<Self> {
        let result = session
            .parse_representation(pending.result.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        session
            .semantic_work()
            .write_bytes(std::mem::size_of::<[FinancialDocument; 3]>())
            .map_err(core_error)?;
        let documents =
            [&pending.root, &pending.authority, &pending.result].map(|r| FinancialDocument {
                reference: r.reference(),
                schema: r.schema(),
                canonical_bytes: r.bytes(),
            });
        Ok(Self {
            pending,
            incoming,
            evidence,
            journal,
            documents,
            result,
        })
    }
    pub(in super::super::super) fn append_view(
        &self,
        session: &mut OriginalAuditSession,
    ) -> Result<PartyAppend<'_>> {
        let body = self.result.value();
        let schema = check::text(body, "schema", session)?;
        let head = if equal_text(schema, "ledger-workflow-party-authorization/1", session)? {
            let authorization = check::field(body, "authorization", session)?;
            Some(PartyHeadEffect {
                key: PartyHeadKey {
                    store: &self.incoming.key().logical_store_id,
                    scope: &self.incoming.key().scope,
                    customer: check::text(authorization, "customer", session)?,
                    agreement: check::text(authorization, "agreement_id", session)?,
                    authorization_id: check::text(authorization, "authorization_id", session)?,
                },
                expected: self.pending.prior_authorization_ref(),
                proposed: self.pending.result.reference(),
            })
        } else {
            require(
                equal_text(schema, "ledger-workflow-party-evidence/1", session)?
                    || equal_text(schema, "ledger-workflow-party-evidence/2", session)?,
                "INTEGRITY_FAILURE",
            )?;
            None
        };
        Ok(PartyAppend {
            logical_store_id: &self.incoming.key().logical_store_id,
            scope: &self.incoming.key().scope,
            source: &self.incoming.key().source,
            operation_id: &self.incoming.key().operation_id,
            documents: &self.documents,
            journal: self.journal.storage_view(
                self.incoming,
                self.evidence,
                self.pending.before,
                self.pending.at.micros(),
                session,
            )?,
            head,
        })
    }
}
