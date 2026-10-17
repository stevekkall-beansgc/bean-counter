//! Borrowable intrinsic JSON representation, never origin/currentness proof.
//! Owned framing establishes only the properties checked by Records::get;
//! per-kind semantic and contextual consumers still execute their paid guards.
use crate::{
    Result,
    canonical::audit_arena::{AuditArena, AuditedValue, ExternalBufferLease},
    validation_work::ValidationWork,
};
use serde::{Serialize, Serializer};
use serde_json::Value;
use std::{mem, ops::Deref, sync::Arc};
pub(super) struct OwningBody {
    value: AuditedValue,
    _arc_payload: ExternalBufferLease,
}
#[derive(Clone)]
pub(super) enum RecordBody {
    // Legacy compatibility keeps its original Value-copy/paid-trace behavior.
    Legacy(Value),
    Owned(Arc<OwningBody>),
}
impl RecordBody {
    /// Exact immutable owning representation identity, not value equality or origin.
    pub(super) fn is_same_owning_value(&self,value:&Value)->bool {
        matches!(self,Self::Owned(v) if std::ptr::eq(v.value.value(),value))
    }
    pub(super) fn owned(
        arena: &AuditArena,
        value: AuditedValue,
        work: &mut dyn ValidationWork,
    ) -> Result<Self> {
        // Arc header + payload is pre-admitted BEFORE Arc::new. No new quota,
        // Serde factory, caller validated bit or naked Value extraction exists.
        let header = crate::validation_work::usize_add(
            work,
            mem::size_of::<OwningBody>(),
            2 * mem::size_of::<usize>(),
        )?;
        let ownership = arena.reserve_external_payload(header, work)?;
        Ok(Self::Owned(Arc::new(OwningBody {
            value,
            _arc_payload: ownership,
        })))
    }
}
impl Deref for RecordBody {
    type Target = Value;
    fn deref(&self) -> &Value {
        match self {
            Self::Legacy(v) => v,
            Self::Owned(v) => v.value.value(),
        }
    }
}
impl Serialize for RecordBody {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.deref().serialize(serializer)
    }
}

#[cfg(test)]
impl std::fmt::Debug for RecordBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.deref().fmt(f)
    }
}
#[cfg(test)]
impl PartialEq<Value> for RecordBody {
    fn eq(&self, other: &Value) -> bool {
        self.deref() == other
    }
}
#[cfg(test)]
impl RecordBody {
    pub(super) fn legacy_mut(&mut self) -> &mut Value {
        match self {
            Self::Legacy(v) => v,
            Self::Owned(_) => panic!("test mutation cannot unwrap an owning artifact"),
        }
    }
}

#[cfg(test)]
impl PartialEq for RecordBody {
    fn eq(&self, other: &Self) -> bool {
        self.deref() == other.deref()
    }
}
