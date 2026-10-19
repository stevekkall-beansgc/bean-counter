//! Interpreter-local mutable payload ownership. RP01 exposes only checked
//! owning replacement/composition, never a naked mutable Value or extraction.
//! The legacy recipe preserves its original copy/trace behavior.
use super::{codec, emitted_record::EncodedBody, shape, CoreMeter};
use crate::{
    canonical::audit_arena::AuditedValue,
    validation_work::{self as w, ArithmeticOperation, RepresentationKind, ValidationWork},
    Result,
};
use serde::{Serialize, Serializer};
use serde_json::Value;
use std::{mem, ops::Deref};
pub(super) enum WorkingValue {
    Legacy(Value),
    Owned(AuditedValue),
}
impl Deref for WorkingValue {
    type Target = Value;
    fn deref(&self) -> &Value {
        match self {
            Self::Legacy(value) => value,
            Self::Owned(value) => value.value(),
        }
    }
}
impl Serialize for WorkingValue {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        self.deref().serialize(s)
    }
}
impl WorkingValue {
    pub(super) fn into_legacy(self) -> Result<Value> {
        match self {
            Self::Legacy(value) => Ok(value),
            Self::Owned(_) => Err(shape::fail(
                "owning value cannot escape through legacy draft",
            )),
        }
    }
    pub(super) fn into_owned(self) -> Result<AuditedValue> {
        match self {
            Self::Owned(value) => Ok(value),
            Self::Legacy(_) => Err(shape::fail("legacy value cannot become an owning artifact")),
        }
    }
    pub(super) fn parse(m: &mut CoreMeter, raw: &[u8]) -> Result<Self> {
        if let Some(arena) = m.representation_arena() {
            let value =
                m.parse_owned_representation(&arena, raw, RepresentationKind::OriginalRequest)?;
            codec::validate_selected_value(m, value.value(), 262144)?;
            Ok(Self::Owned(value))
        } else {
            Ok(Self::Legacy(codec::parse(m, raw, 262144)?))
        }
    }
    pub(super) fn copy(m: &mut CoreMeter, value: &Value) -> Result<Self> {
        if let Some(arena) = m.representation_arena() {
            Ok(Self::Owned(arena.clone_value(value, m)?))
        } else {
            Ok(Self::Legacy(value.clone()))
        }
    }
    pub(super) fn replace(&mut self, m: &mut CoreMeter, key: &str, value: &Value) -> Result<()> {
        match self {
            Self::Legacy(current) => {
                current[key] = value.clone();
                Ok(())
            }
            Self::Owned(current) => {
                let arena = m
                    .representation_arena()
                    .ok_or_else(|| shape::fail("working payload owner"))?;
                let value = arena.clone_value(value, m)?;
                current.replace_existing_owned(key, value, m)
            }
        }
    }
    pub(super) fn with_fields(&self, m: &mut CoreMeter, extra: &[(&str, &Value)]) -> Result<Self> {
        match self {
            Self::Legacy(value) => {
                let mut value = value.clone();
                for (key, field) in extra {
                    value[*key] = (*field).clone();
                }
                Ok(Self::Legacy(value))
            }
            Self::Owned(value) => {
                let arena = m
                    .representation_arena()
                    .ok_or_else(|| shape::fail("working composition owner"))?;
                let object = value
                    .value()
                    .as_object()
                    .ok_or_else(|| shape::fail("working composition object"))?;
                let n = w::usize_add(m, object.len(), extra.len())?;
                let item = mem::size_of::<(&str, &Value)>();
                ValidationWork::arithmetic(
                    m,
                    ArithmeticOperation::Multiply,
                    &[&n.into(), &item.into()],
                )?;
                let size = n
                    .checked_mul(item)
                    .ok_or_else(|| shape::fail("working composition metadata"))?;
                let size = w::usize_add(m, size, mem::size_of::<Vec<(&str, &Value)>>())?;
                let _metadata = arena.reserve_external_payload(size, m)?;
                // Prepay control/element writes before allocation and before
                // any field validation can refuse partway through the fill.
                m.write_bytes(size)?;
                let mut fields = Vec::new();
                fields
                    .try_reserve_exact(n)
                    .map_err(|_| shape::fail("working composition allocation"))?;
                for (key, field) in object {
                    m.visit("working.original_field")?;
                    let mut replaced = false;
                    for (other, _) in extra {
                        if shape::same_text(m, key, other)? {
                            replaced = true;
                            break;
                        }
                    }
                    if !replaced {
                        fields.push((key.as_str(), field));
                    }
                }
                for field in extra {
                    m.visit("working.replacement_field")?;
                    fields.push(*field);
                }
                Ok(Self::Owned(arena.object_from_fields(&fields, m)?))
            }
        }
    }
    pub(super) fn replace_reference_array_field(
        &mut self,
        m: &mut CoreMeter,
        array: &str,
        index: usize,
        key: &str,
        refs: &[super::data::ExactRef],
    ) -> Result<()> {
        match self {
            Self::Legacy(value) => {
                value[array][index][key] = serde_json::json!(refs);
                Ok(())
            }
            Self::Owned(value) => {
                let arena = m
                    .representation_arena()
                    .ok_or_else(|| shape::fail("working reference set owner"))?;
                let per = mem::size_of::<AuditedValue>() + mem::size_of::<&Value>();
                ValidationWork::arithmetic(
                    m,
                    ArithmeticOperation::Multiply,
                    &[&refs.len().into(), &per.into()],
                )?;
                let size = refs
                    .len()
                    .checked_mul(per)
                    .ok_or_else(|| shape::fail("working reference set metadata"))?;
                let _metadata = arena.reserve_external_payload(size, m)?;
                let mut owned = Vec::new();
                owned
                    .try_reserve_exact(refs.len())
                    .map_err(|_| shape::fail("working reference set allocation"))?;
                for reference in refs {
                    m.visit("working.reference_member")?;
                    owned.push(owning_reference(m, &arena, reference)?);
                }
                let mut borrowed = Vec::new();
                borrowed
                    .try_reserve_exact(refs.len())
                    .map_err(|_| shape::fail("working reference borrow allocation"))?;
                for reference in &owned {
                    m.visit("working.reference_borrow")?;
                    borrowed.push(reference.value());
                }
                let replacement = arena.array_from_values(&borrowed, m)?;
                value.replace_existing_array_field_owned(array, index, key, replacement, m)
            }
        }
    }
    pub(super) fn bytes(&self, m: &mut CoreMeter) -> Result<EncodedBody> {
        match self {
            Self::Legacy(value) => Ok(EncodedBody::Legacy(codec::encode(m, value, 262144)?)),
            Self::Owned(value) => {
                let arena = m
                    .representation_arena()
                    .ok_or_else(|| shape::fail("working bytes owner"))?;
                let bytes = m.canonical_owned_representation(
                    &arena,
                    value.value(),
                    RepresentationKind::OriginalRequest,
                )?;
                EncodedBody::owned(&arena, bytes, m)
            }
        }
    }
    pub(super) fn normalize_set(
        &mut self,
        m: &mut CoreMeter,
        key: &str,
        field_key: bool,
    ) -> Result<()> {
        match self {
            Self::Owned(value) => {
                let arena = m
                    .representation_arena()
                    .ok_or_else(|| shape::fail("working set owner"))?;
                super::intent::sort_owned_set(value, key, field_key, m, &arena)
            }
            Self::Legacy(value) => {
                if field_key {
                    super::outcome::normalize_proofs(m, value)
                } else {
                    super::root_correction::normalize_object_field(m, value, key)
                }
            }
        }
    }
}

pub(super) enum NormalizedInput {
    Legacy(crate::policy::workflow::NormalizedFacts),
    Owned(super::owned_facts::OwnedNormalizedFacts),
}
impl Deref for NormalizedInput {
    type Target = crate::policy::workflow::NormalizedFacts;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Legacy(v) => v,
            Self::Owned(v) => v.facts(),
        }
    }
}
impl NormalizedInput {
    pub(super) fn normalize(
        m: &mut CoreMeter,
        schema: &crate::policy::workflow::FactSchema,
        value: &Value,
    ) -> Result<Self> {
        if let Some(arena) = m.representation_arena() {
            Ok(Self::Owned(super::owned_facts::normalize(
                &arena, m, schema, value,
            )?))
        } else {
            Ok(Self::Legacy(schema.normalize_value_with_work(value, m)?))
        }
    }
}

pub(super) fn owning_reference(
    m: &mut CoreMeter,
    arena: &crate::canonical::audit_arena::AuditArena,
    r: &super::data::ExactRef,
) -> Result<AuditedValue> {
    let kind = arena.string_value(&r.kind, m)?;
    let id = arena.string_value(&r.id, m)?;
    let digest = arena.string_value(&r.digest, m)?;
    arena.object_from_fields(
        &[
            ("kind", kind.value()),
            ("id", id.value()),
            ("digest", digest.value()),
        ],
        m,
    )
}

/// Definition references use the same closed three-string representation.
/// This emitter owns copies only; it confers no completed publication fact.
pub(super) fn owning_definition(
    m: &mut CoreMeter,
    arena: &crate::canonical::audit_arena::AuditArena,
    r: &super::super::DefinitionRef,
) -> Result<AuditedValue> {
    let kind = arena.string_value(r.kind(), m)?;
    let id = arena.string_value(r.id(), m)?;
    let digest = arena.string_value(r.digest(), m)?;
    arena.object_from_fields(
        &[
            ("kind", kind.value()),
            ("id", id.value()),
            ("digest", digest.value()),
        ],
        m,
    )
}

/// A typed Ref copy whose three String payloads are admitted before copying.
/// Borrowing this object does not prove origin or grant current authority.
pub(super) struct OwnedExactRef {
    reference: super::data::ExactRef,
    _ownership: crate::canonical::audit_arena::ExternalBufferLease,
}
impl OwnedExactRef {
    pub(super) fn reference(&self) -> &super::data::ExactRef {
        &self.reference
    }
}
pub(super) fn parse_owned_reference(
    m: &mut CoreMeter,
    arena: &crate::canonical::audit_arena::AuditArena,
    value: &Value,
) -> Result<OwnedExactRef> {
    shape::closed(m, value, &["kind", "id", "digest"], &[])?;
    let kind = shape::text(m, value, "kind")?;
    let id = shape::text(m, value, "id")?;
    let digest = shape::text(m, value, "digest")?;
    super::records::reference_parts(m, kind, id, digest)?;
    let n = w::usize_add(m, mem::size_of::<OwnedExactRef>(), kind.len())?;
    let n = w::usize_add(m, n, id.len())?;
    let n = w::usize_add(m, n, digest.len())?;
    let ownership = arena.reserve_external_payload(n, m)?;
    for s in [kind, id, digest] {
        m.scan_bytes(s.len())?;
        m.write_bytes(s.len())?;
    }
    Ok(OwnedExactRef {
        reference: super::data::ExactRef {
            kind: kind.to_owned(),
            id: id.to_owned(),
            digest: digest.to_owned(),
        },
        _ownership: ownership,
    })
}

/// Typed original OperationKey parsing with retained string/control ownership.
/// Exact captured bytes and historical authority remain the caller's joins.
pub(super) struct WorkingKey {
    key: super::data::OperationKey,
    _owner: Option<crate::canonical::audit_arena::ExternalBufferLease>,
}
impl Deref for WorkingKey {
    type Target = super::data::OperationKey;
    fn deref(&self) -> &Self::Target {
        &self.key
    }
}
impl Serialize for WorkingKey {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        self.key.serialize(s)
    }
}
impl WorkingKey {
    pub(super) fn parse(m: &mut CoreMeter, value: &Value) -> Result<Self> {
        let Some(arena) = m.representation_arena() else {
            let key = serde_json::from_value(value.clone())
                .map_err(|_| shape::fail("original work key"))?;
            return Ok(Self { key, _owner: None });
        };
        shape::closed(
            m,
            value,
            &["logical_store_id", "scope", "source", "operation_id"],
            &[],
        )?;
        let store = shape::text(m, value, "logical_store_id")?;
        let source = shape::text(m, value, "source")?;
        let id = shape::text(m, value, "operation_id")?;
        let scope = shape::array(m, value, "scope")?;
        if !w::compare(m, scope.len(), 2usize)?.is_eq() {
            return Err(shape::fail("original work key scope"));
        }
        m.visit("working.original_scope_element")?;
        let first = scope[0]
            .as_str()
            .ok_or_else(|| shape::fail("original work key scope"))?;
        m.visit("working.original_scope_element")?;
        let second = scope[1]
            .as_str()
            .ok_or_else(|| shape::fail("original work key scope"))?;
        let mut bytes = mem::size_of::<Self>();
        for raw in [store, first, second, source, id] {
            bytes = w::usize_add(m, bytes, raw.len())?;
        }
        let owner = arena.reserve_external_payload(bytes, m)?;
        m.write_bytes(bytes)?;
        for raw in [store, first, second, source, id] {
            m.scan_bytes(raw.len())?;
        }
        Ok(Self {
            key: super::data::OperationKey {
                logical_store_id: store.to_owned(),
                scope: [first.to_owned(), second.to_owned()],
                source: source.to_owned(),
                operation_id: id.to_owned(),
            },
            _owner: Some(owner),
        })
    }
}
