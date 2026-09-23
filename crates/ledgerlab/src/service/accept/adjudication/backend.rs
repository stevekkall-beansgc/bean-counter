//! Narrow trusted-backend construction boundary. No type here is a wire DTO.
//! Callers must hold the actual primary transaction/storage guard; the checks
//! below bind that observation and do not substitute for acquiring the guard.
use super::*;
use ledgerlab_core::adjudication::{self as r3, Validate};
use ledgerlab_core::{Error, Result};
fn require(ok: bool, detail: &'static str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error {
            code: "BACKEND_BINDING",
            detail: detail.into(),
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TrustedJournalHead {
    journal: JournalIdentity,
    ordinal: Count,
    segment: Digest,
    root: Digest,
    observation: Digest,
}
impl TrustedJournalHead {
    pub(crate) fn from_backend(
        journal: JournalIdentity,
        ordinal: Count,
        segment: Digest,
        root: Digest,
        observation: Digest,
    ) -> Result<Self> {
        let zero = "0".repeat(64);
        require(
            if ordinal == Count::ZERO {
                segment.as_str() == zero && root.as_str() == zero
            } else {
                segment.as_str() != zero && root.as_str() != zero
            },
            "genesis ordinal/head",
        )?;
        Ok(Self {
            journal,
            ordinal,
            segment,
            root,
            observation,
        })
    }
    pub(crate) fn journal(&self) -> &JournalIdentity {
        &self.journal
    }
    pub(crate) fn ordinal(&self) -> Count {
        self.ordinal
    }
    pub(crate) fn segment(&self) -> &Digest {
        &self.segment
    }
    pub(crate) fn root(&self) -> &Digest {
        &self.root
    }
    pub(crate) fn observation(&self) -> &Digest {
        &self.observation
    }
}
impl TrustedPrefix {
    /// The adapter must first authenticate the enrolled binding from its primary
    /// snapshot. A command's proposed target/enrollment does not supply it.
    pub(crate) fn from_backend(
        head: &TrustedJournalHead,
        expected: wire::ExpectedPrefix,
        selection: PrefixSelection,
    ) -> Result<Self> {
        expected.validate()?;
        require(
            expected.store == head.journal.store
                && expected.scope == head.journal.scope
                && expected.registration == head.journal.registration
                && expected.host == head.journal.host
                && expected.ordinal == head.ordinal
                && expected.segment == head.segment
                && expected.root == head.root,
            "enrolled prefix journal binding",
        )?;
        require(
            expected.enrollment.as_str() != "0".repeat(64),
            "enrolled prefix must have enrollment",
        )?;
        Ok(Self {
            expected,
            observation: head.observation.clone(),
            selection,
        })
    }
}
impl PhysicalEnvelope {
    /// All quantities are enforced backend bounds, supplied after live admission.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_backend(
        maximum_retained_bytes: Count,
        maximum_index_pages: Count,
        maximum_wal_bytes: Count,
        maximum_staging_bytes: Count,
        maximum_reader_retention_bytes: Count,
        protected_workspace_bytes: Count,
        enforcement_observation: Digest,
    ) -> Result<Self> {
        require(
            maximum_staging_bytes.value() >= r3::SEGMENT_BYTES as u128
                && protected_workspace_bytes.value() >= r3::SEGMENT_BYTES as u128,
            "complete segment staging/work lane",
        )?;
        Ok(Self {
            maximum_retained_bytes,
            maximum_index_pages,
            maximum_wal_bytes,
            maximum_staging_bytes,
            maximum_reader_retention_bytes,
            protected_workspace_bytes,
            enforcement_observation,
        })
    }
    pub(crate) fn same_enforcement(&self, other: &Self) -> bool {
        self.maximum_retained_bytes() == other.maximum_retained_bytes()
            && self.maximum_index_pages() == other.maximum_index_pages()
            && self.maximum_wal_bytes() == other.maximum_wal_bytes()
            && self.maximum_staging_bytes() == other.maximum_staging_bytes()
            && self.maximum_reader_retention_bytes() == other.maximum_reader_retention_bytes()
            && self.protected_workspace_bytes() == other.protected_workspace_bytes()
            && self.enforcement_observation() == other.enforcement_observation()
    }
    pub(crate) fn maximum_retained_bytes(&self) -> Count {
        self.maximum_retained_bytes
    }
    pub(crate) fn maximum_index_pages(&self) -> Count {
        self.maximum_index_pages
    }
    pub(crate) fn maximum_wal_bytes(&self) -> Count {
        self.maximum_wal_bytes
    }
    pub(crate) fn maximum_staging_bytes(&self) -> Count {
        self.maximum_staging_bytes
    }
    pub(crate) fn maximum_reader_retention_bytes(&self) -> Count {
        self.maximum_reader_retention_bytes
    }
    pub(crate) fn protected_workspace_bytes(&self) -> Count {
        self.protected_workspace_bytes
    }
    pub(crate) fn enforcement_observation(&self) -> &Digest {
        &self.enforcement_observation
    }
}
impl CommitCapability {
    /// The live Tx retains exclusion/work guards through rollback/commit/drop.
    /// Append must compare all these bindings against that same live Tx again.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_backend(
        journal: JournalIdentity,
        transaction: Digest,
        storage_incarnation: Digest,
        writer_epoch: Count,
        recovered_through: TrustedJournalHead,
        allocation_owner: Id,
        physical: PhysicalEnvelope,
        resource_ceiling: wire::Resource,
        writer_fence: Option<Digest>,
    ) -> Result<Self> {
        resource_ceiling.validate()?;
        require(
            journal == *recovered_through.journal(),
            "capability recovered journal",
        )?;
        require(
            transaction.as_str() != "0".repeat(64)
                && storage_incarnation.as_str() != "0".repeat(64),
            "actual transaction and storage incarnation",
        )?;
        Ok(Self {
            journal,
            transaction,
            storage_incarnation,
            writer_epoch,
            recovered_through,
            allocation_owner,
            physical,
            resource_ceiling,
            writer_fence,
        })
    }
}
impl VerifiedSource {
    /// `head` is an actual independently selected source-journal observation.
    /// Source store must establish committed membership and a locally verified
    /// semantic cursor through this head; a transport body cannot mint `head`.
    pub(crate) fn from_backend(
        head: TrustedJournalHead,
        proof: wire::Proof,
        segment_bytes: &[u8],
    ) -> Result<Self> {
        proof.validate()?;
        require(
            proof.trusted_observation_ref == head.observation,
            "source trusted observation",
        )?;
        let segment: r3::commands::Segment = r3::parse_exact(segment_bytes, r3::SEGMENT_BYTES)?;
        require(
            proof.store == head.journal.store
                && proof.scope == head.journal.scope
                && proof.registration == head.journal.registration
                && proof.host == head.journal.host
                && proof.ordinal == head.ordinal
                && proof.segment == head.segment
                && proof.root == head.root,
            "source head proof binding",
        )?;
        require(
            segment.host == proof.host
                && segment.ordinal == proof.ordinal
                && segment.result.root == proof.root
                && r3::runtime::hash("segment", &segment)? == proof.segment,
            "source segment membership",
        )?;
        let command = r3::runtime::command_value(&segment.command)?;
        let mut authorizing_target = None;
        for object in &segment.objects {
            if object.kind == wire::FactKind::Authority
                && command["authority"]["document"] == object.body_hash.as_str()
            {
                let raw = r3::proofs::VerifiedObjectBytes::check(object.clone())?;
                let body: wire::AuthoritySourceBody = r3::parse_exact(raw.bytes(), 16384)?;
                let body = serde_json::to_value(body).map_err(|_| Error {
                    code: "AUTH_SOURCE",
                    detail: "serialization".into(),
                })?;
                require(
                    body["kind"] == "AUTHORIZATION"
                        && body["scope"] == command["key"][0]
                        && body["principal"] == command["authority"]["principal"]
                        && body["revision"] == command["authority"]["revision"],
                    "source authority body",
                )?;
                authorizing_target =
                    Some(Id::parse(body["target"].as_str().ok_or_else(|| {
                        Error {
                            code: "AUTH_TARGET",
                            detail: "source target".into(),
                        }
                    })?)?);
            }
        }
        let pkey = r3::canonical_bytes(&proof.full_key, r3::COMMAND_BYTES)?;
        let mut found = None;
        for object in segment.objects {
            if object.kind == proof.fact_kind
                && r3::canonical_bytes(&object.full_key, r3::COMMAND_BYTES)? == pkey
            {
                require(found.is_none(), "ambiguous source fact")?;
                require(
                    object.origin.store == proof.store
                        && object.origin.scope == proof.scope
                        && object.origin.registration == proof.registration
                        && object.origin.host == proof.host
                        && object.origin.ordinal == proof.ordinal
                        && object.body_hash == proof.body_hash
                        && object.bytes == proof.bytes,
                    "exact source object identity",
                )?;
                r3::proofs::VerifiedObjectBytes::check(object.clone())?;
                found = Some(object);
            }
        }
        let exact_object = found.ok_or_else(|| Error {
            code: "PROOF_MEMBER",
            detail: "source fact missing".into(),
        })?;
        Ok(Self {
            prefix: head,
            proof,
            exact_object,
            authorizing_target,
        })
    }
}
