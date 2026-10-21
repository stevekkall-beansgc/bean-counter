//! Private interpreter output representation. Owning bytes/decoded framing are
//! retained through capacity/funding and final candidate assembly. This does
//! not cover upstream json!/reference/vector producers or confer admission.
use super::{
    codec,
    data::{DraftEnvelope, ExactRef},
    record_body::RecordBody,
    shape, CoreMeter,
};
use crate::{
    canonical::audit_arena::{AuditArena, AuditedBytes, ExternalBufferLease},
    validation_work::{self as w, RepresentationKind},
    Result,
};
use std::{mem, ops::Deref, sync::Arc};

pub(super) struct OwnedEncoding {
    bytes: AuditedBytes,
    // Arc allocation metadata is separately prepaid; byte payload precedes it.
    _metadata: ExternalBufferLease,
}
#[derive(Clone)]
pub(super) enum EncodedBody {
    Legacy(Vec<u8>),
    Owned(Arc<OwnedEncoding>),
}
impl Deref for EncodedBody {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Legacy(v) => v,
            Self::Owned(v) => v.bytes.bytes(),
        }
    }
}
impl EncodedBody {
    pub(super) fn owned(
        arena: &AuditArena,
        bytes: AuditedBytes,
        m: &mut CoreMeter,
    ) -> Result<Self> {
        let metadata = w::usize_add(
            m,
            mem::size_of::<OwnedEncoding>(),
            2 * mem::size_of::<usize>(),
        )?;
        let lease = arena.reserve_external_payload(metadata, m)?;
        Ok(Self::Owned(Arc::new(OwnedEncoding {
            bytes,
            _metadata: lease,
        })))
    }
    pub(super) fn into_legacy(self) -> Result<Vec<u8>> {
        match self {
            Self::Legacy(v) => Ok(v),
            Self::Owned(_) => Err(shape::fail(
                "owning output cannot escape through legacy draft",
            )),
        }
    }
}
#[derive(Clone)]
pub(super) struct ConstructedEnvelope {
    pub reference: ExactRef,
    pub canonical_body: EncodedBody,
    // A producer's canonical bytes are parsed exactly once on RP01. Consumers
    // still execute their actual semantic checks on this borrowed framing.
    pub decoded: Option<RecordBody>,
}
impl ConstructedEnvelope {
    pub(super) fn from_bytes(reference: ExactRef, canonical_body: Vec<u8>) -> Self {
        Self {
            reference,
            canonical_body: EncodedBody::Legacy(canonical_body),
            decoded: None,
        }
    }
    pub(super) fn from_owned(
        reference: ExactRef,
        canonical_body: AuditedBytes,
        arena: &AuditArena,
        m: &mut CoreMeter,
        kind: RepresentationKind,
    ) -> Result<Self> {
        let decoded = m.parse_owned_representation(arena, canonical_body.bytes(), kind)?;
        let decoded = RecordBody::owned(arena, decoded, m)?;
        let canonical_body = EncodedBody::owned(arena, canonical_body, m)?;
        Ok(Self {
            reference,
            canonical_body,
            decoded: Some(decoded),
        })
    }
    pub(super) fn from_encoded(
        reference: ExactRef,
        canonical_body: EncodedBody,
        arena: Option<&AuditArena>,
        m: &mut CoreMeter,
        kind: RepresentationKind,
    ) -> Result<Self> {
        let decoded = if let Some(arena) = arena {
            if !matches!(canonical_body, EncodedBody::Owned(_)) {
                return Err(shape::fail("unowned output on owning interpreter path"));
            }
            let parsed = m.parse_owned_representation(arena, &canonical_body, kind)?;
            Some(RecordBody::owned(arena, parsed, m)?)
        } else {
            None
        };
        Ok(Self {
            reference,
            canonical_body,
            decoded,
        })
    }
    pub(super) fn decoded(&self, m: &mut CoreMeter, maximum: usize) -> Result<RecordBody> {
        match &self.decoded {
            Some(value) => Ok(value.clone()),
            None => Ok(RecordBody::Legacy(codec::parse(
                m,
                &self.canonical_body,
                maximum,
            )?)),
        }
    }
    pub(super) fn into_legacy(self) -> Result<DraftEnvelope> {
        Ok(DraftEnvelope {
            reference: self.reference,
            canonical_body: self.canonical_body.into_legacy()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{body, data::OperationKey};
    use super::*;
    use crate::validation_work::OriginalAuditSession;
    use serde_json::json;
    #[test]
    fn owned_output_retains_exact_legacy_bytes_and_reuses_its_frame() {
        let key = OperationKey {
            logical_store_id: "store".into(),
            scope: ["a".into(), "b".into()],
            source: "source".into(),
            operation_id: "operation".into(),
        };
        let literal =
            json!({"schema":"ledger-workflow-chain-state/1","literal":"same original bytes"});
        let id = format!("wchain_{}", "1".repeat(64));
        let mut old = CoreMeter::new(65536).unwrap();
        let legacy = body::record(
            &mut old,
            &key,
            "workflow-chain-state",
            id.clone(),
            literal.clone(),
        )
        .unwrap();
        let arena = AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut m = session.meter_mut().original_economic_meter().unwrap();
        m.enter_draft().unwrap();
        let emitted = body::record(&mut m, &key, "workflow-chain-state", id, literal).unwrap();
        assert_eq!(emitted.reference, legacy.reference);
        assert_eq!(&*emitted.canonical_body, &*legacy.canonical_body);
        let once = m.resource_vector_usage().unwrap().representation_calls;
        let first = emitted.decoded(&mut m, 4194304).unwrap();
        let second = emitted.decoded(&mut m, 4194304).unwrap();
        assert!(std::ptr::eq(first.deref(), second.deref()));
        assert_eq!(
            m.resource_vector_usage().unwrap().representation_calls,
            once
        );
        assert!(emitted.into_legacy().is_err());
        assert!(arena.usage().unwrap().live_bytes > baseline);
        drop((first, second));
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }
    #[test]
    fn aggregate_new_body_bound_caps_the_owning_emitter_before_output() {
        let arena = AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut m = session.meter_mut().original_economic_meter().unwrap();
        m.enter_draft().unwrap();
        // Fill only the existing logical new-body dimension. The attempted
        // string is synthetic pure test input, never a financial request.
        m.new_body(8 * 1024 * 1024 - 8).unwrap();
        let emitted = body::document(
            &mut m,
            "workflow-economic-authority",
            json!({"schema":"longer-than-eight"}),
        );
        assert!(emitted.is_err());
        assert_eq!(m.usage().new_body_bytes, 8 * 1024 * 1024 - 8);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        assert!(session.meter_mut().finish_original_economic(m).is_err());
    }
}
