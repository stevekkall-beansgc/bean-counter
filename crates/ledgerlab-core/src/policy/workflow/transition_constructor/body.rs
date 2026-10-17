//! Acyclic draft envelope construction. These records remain noncommittable;
//! the authority-data resolver supplies the immutable original slot operands.
use super::emitted_record::ConstructedEnvelope;
use super::{
    CoreMeter, codec,
    data::{ExactRef, OperationKey},
    records,
};
use crate::{Error, Result};
use serde_json::{Value, json};
pub(super) fn semantic_id(
    m: &mut CoreMeter,
    prefix: &str,
    domain: &str,
    key: &Value,
) -> Result<String> {
    let digest = records::hash(m, domain, key)?;
    m.scan_bytes(digest.len() - 7)?;
    m.write_bytes(prefix.len() + digest.len() - 7)?;
    Ok(format!("{prefix}{}", &digest[7..]))
}
pub(super) fn document(m: &mut CoreMeter, kind: &str, body: Value) -> Result<ConstructedEnvelope> {
    document_borrowed(m, kind, &body)
}
pub(super) fn document_borrowed(m: &mut CoreMeter, kind: &str, body: &Value) -> Result<ConstructedEnvelope> {
    let schema = body["schema"]
        .as_str()
        .ok_or_else(|| Error::new("INTEGRITY_FAILURE", "draft schema"))?;
    let authority = super::shape::same_text(m, kind, "workflow-economic-authority")?
        || super::shape::same_text(m, kind, "workflow-authority")?;
    let maximum = if authority { 65536 } else { 4194304 };
    if let Some(arena) = m.representation_arena() {
        let class = if authority {
            crate::validation_work::RepresentationKind::Authority
        } else {
            crate::validation_work::RepresentationKind::Record
        };
        let bytes = m.canonical_new_body_representation(&arena, &body, class)?;
        m.new_body(bytes.bytes().len())?;
        let schema_bytes = m.canonical_owned_representation(
            &arena,
            &body["schema"],
            crate::validation_work::RepresentationKind::SourceDefinition,
        )?;
        let digest = records::hash_parts(
            m,
            "workflow-document",
            &[schema_bytes.bytes(), bytes.bytes()],
            true,
        )?;
        let reference = ExactRef {
            kind: kind.to_owned(),
            id: format!("wdoc_{}", &digest[7..]),
            digest,
        };
        return ConstructedEnvelope::from_owned(reference, bytes, &arena, m, class);
    }
    let bytes = codec::encode_new(m, &body, maximum)?;
    let schema_bytes = codec::encode(m, &json!(schema), 262144)?;
    let digest = records::hash_parts(m, "workflow-document", &[&schema_bytes, &bytes], true)?;
    Ok(ConstructedEnvelope::from_bytes(
        ExactRef {
            kind: kind.to_owned(),
            id: format!("wdoc_{}", &digest[7..]),
            digest,
        },
        bytes,
    ))
}
pub(super) fn record(
    m: &mut CoreMeter,
    key: &OperationKey,
    kind: &str,
    id: String,
    body: Value,
) -> Result<ConstructedEnvelope> {
    record_borrowed(m,key,kind,id,&body)
}
pub(super) fn record_borrowed(
    m:&mut CoreMeter,key:&OperationKey,kind:&str,id:String,body:&Value,
) -> Result<ConstructedEnvelope> {
    if let Some(arena) = m.representation_arena() {
        let class = crate::validation_work::RepresentationKind::Record;
        let bytes = m.canonical_new_body_representation(&arena, &body, class)?;
        m.new_body(bytes.bytes().len())?;
        // Caller control-graph pre-admission must cover these short frame
        // values/Ref strings; returned bytes and decoded bodies own their leases.
        let kind_bytes = m.canonical_owned_representation(
            &arena,
            &json!(kind),
            crate::validation_work::RepresentationKind::SourceDefinition,
        )?;
        let scope = m.canonical_owned_representation(
            &arena,
            &json!(key.scope),
            crate::validation_work::RepresentationKind::SourceDefinition,
        )?;
        let digest = records::hash_parts(
            m,
            "workflow-record",
            &[kind_bytes.bytes(), scope.bytes(), bytes.bytes()],
            true,
        )?;
        return ConstructedEnvelope::from_owned(
            ExactRef {
                kind: kind.to_owned(),
                id,
                digest,
            },
            bytes,
            &arena,
            m,
            class,
        );
    }
    let bytes = codec::encode_new(m, &body, 4194304)?;
    let kind_bytes = codec::encode(m, &json!(kind), 262144)?;
    let scope = codec::encode(m, &json!(key.scope), 262144)?;
    let digest = records::hash_parts(m, "workflow-record", &[&kind_bytes, &scope, &bytes], true)?;
    Ok(ConstructedEnvelope::from_bytes(
        ExactRef {
            kind: kind.to_owned(),
            id,
            digest,
        },
        bytes,
    ))
}
pub(super) fn ref_set(m: &mut CoreMeter, refs: Vec<ExactRef>) -> Result<Vec<ExactRef>> {
    let mut result = Vec::<(Vec<u8>, ExactRef)>::new();
    for reference in refs {
        let value = serde_json::to_value(&reference)
            .map_err(|_| Error::new("INTEGRITY_FAILURE", "reference encoding"))?;
        let bytes = codec::encode(m, &value, 262144)?;
        let mut position = result.len();
        let mut duplicate = false;
        for (i, (prior, _)) in result.iter().enumerate() {
            m.visit("refset.compare")?;
            match super::shape::byte_order(m, &bytes, prior)? {
                std::cmp::Ordering::Equal => {
                    duplicate = true;
                    break;
                }
                std::cmp::Ordering::Less => {
                    position = i;
                    break;
                }
                _ => {}
            }
        }
        if !duplicate {
            m.visit("refset.insert")?;
            for _ in position..result.len() {
                m.visit("refset.move")?;
            }
            result.insert(position, (bytes, reference));
        }
    }
    Ok(result.into_iter().map(|(_, r)| r).collect())
}

/// Generic named object sets are normalized by canonical JCS bytes. Semantic
/// identity duplication (for example repeated family states) is checked before
/// this representation normalization; an identical whole object also refuses.
pub(super) fn object_set(m: &mut CoreMeter, values: Vec<Value>) -> Result<Vec<Value>> {
    let mut ordered: Vec<(Vec<u8>, Value)> = vec![];
    for value in values {
        m.visit("object_set.element")?;
        let key = codec::encode(m, &value, 4194304)?;
        let mut position = ordered.len();
        for (i, (prior, _)) in ordered.iter().enumerate() {
            match super::shape::byte_order(m, &key, prior)? {
                std::cmp::Ordering::Equal => {
                    return Err(Error::new("DUPLICATE", "generic object set member"));
                }
                std::cmp::Ordering::Less => {
                    position = i;
                    break;
                }
                std::cmp::Ordering::Greater => {}
            }
        }
        for _ in position..ordered.len() {
            m.visit("object_set.move")?;
        }
        ordered.insert(position, (key, value));
    }
    Ok(ordered.into_iter().map(|(_, value)| value).collect())
}
