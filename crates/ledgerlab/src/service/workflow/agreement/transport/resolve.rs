//! Exact typed source closure resolution. Historical normalization never loads
//! current support or mutable source heads, and never constructs a compiler token.
use super::*;
pub(super) struct Resolution {
    pub input: core::SourceRetentionInput,
    pub payload: Value,
    pub definition: Option<core::NormalizedDefinition>,
    pub fragments: Vec<Document>,
    pub dependencies: Vec<Document>,
}
pub(super) async fn resolve(
    tx: &mut SqliteTx,
    key: &OperationKey,
    payload: &Value,
    fresh: bool,
) -> Result<Resolution> {
    let input =
        core::SourceRetentionInput::parse(&records::bytes(payload)?).map_err(transport_error)?;
    if let Some(fragment) = input.fragment() {
        return Ok(Resolution {
            payload: fragment.canonical_value().map_err(transport_error)?,
            input,
            definition: None,
            fragments: vec![],
            dependencies: vec![],
        });
    }
    let mut fragments = vec![];
    let raw = if let Some(raw) = input.candidate_document_bytes() {
        raw.to_vec()
    } else {
        let finalize = input.finalization().ok_or(ServiceError::IntegrityFailure)?;
        let mut parts = vec![];
        for reference in finalize.fragment_refs() {
            let r = record_ref(reference)?;
            if fresh {
                Box::pin(verify::document_origin(tx, key, &r)).await?;
            }
            let d = loaded(tx, key, &r).await?;
            canonical_body(&d)?;
            let part = core::DefinitionFragment::parse(&d.bytes)
                .map_err(|_| ServiceError::IntegrityFailure)?;
            if record_ref(part.reference())? != d.reference {
                return Err(ServiceError::IntegrityFailure);
            }
            parts.push(part);
            fragments.push(d);
        }
        finalize
            .reassemble(&parts, &key.logical_store_id, &key.scope, &key.source)
            .map_err(|e| {
                if e.code == "DEFINITION_TRANSPORT" {
                    reject("SOURCE_FRAGMENT_MISMATCH")
                } else {
                    transport_error(e)
                }
            })?
    };
    let mut pending = core::candidate_dependency_refs(&raw).map_err(transport_error)?;
    let mut documents = std::collections::BTreeMap::<String, Document>::new();
    let mut total = raw.len();
    while let Some(reference) = pending.pop() {
        let r = record_ref(&reference)?;
        require(source_kind(&r.kind), "INVALID_REFERENCE")?;
        if let Some(previous) = documents.get(&r.id) {
            if previous.reference != r {
                return Err(ServiceError::IntegrityFailure);
            }
            continue;
        }
        require(documents.len() < 127, "PROGRAM_BUDGET_EXCEEDED")?;
        if fresh {
            Box::pin(verify::document_origin(tx, key, &r)).await?;
        }
        let d = loaded(tx, key, &r).await?;
        canonical_body(&d)?;
        require(d.bytes.len() <= 262144, "PROGRAM_BUDGET_EXCEEDED")?;
        total = total
            .checked_add(d.bytes.len())
            .ok_or(ServiceError::IntegrityFailure)?;
        require(total <= 1048576, "PROGRAM_BUDGET_EXCEEDED")?;
        pending.extend(core::candidate_dependency_refs(&d.bytes).map_err(transport_error)?);
        documents.insert(r.id, d);
    }
    let definition = normalize(&raw, &documents)?;
    if input.kind() == "finalize" {
        require(
            definition.document().canonical_bytes() == raw,
            "SOURCE_FRAGMENT_MISMATCH",
        )?;
    }
    if fresh {
        let support = core::VerifiedCompilerSupport::builtin().map_err(transport_error)?;
        core::normalize_definition(&raw, definition.dependencies(), &support)
            .map_err(transport_error)?;
    }
    let normalized = if input.kind() == "direct" {
        json!({"schema":"ledger-workflow-definition-retain/1","document":definition.document().canonical_value()})
    } else {
        input
            .finalization()
            .ok_or(ServiceError::IntegrityFailure)?
            .canonical_value()
    };
    Ok(Resolution {
        input,
        payload: normalized,
        definition: Some(definition),
        fragments,
        dependencies: documents.into_values().collect(),
    })
}
fn normalize(
    raw: &[u8],
    documents: &std::collections::BTreeMap<String, Document>,
) -> Result<core::NormalizedDefinition> {
    let mut typed = std::collections::BTreeMap::<String, core::DefinitionDocument>::new();
    while typed.len() < documents.len() {
        let mut progress = false;
        for (id, d) in documents {
            if typed.contains_key(id) {
                continue;
            }
            let refs = core::candidate_dependency_refs(&d.bytes).map_err(transport_error)?;
            if refs
                .iter()
                .any(|r| !typed.get(r.id()).is_some_and(|d| d.reference() == r))
            {
                continue;
            }
            let closure = dependencies(&refs, &typed)?;
            let normalized =
                core::normalize_retained_definition(&d.bytes, &closure).map_err(transport_error)?;
            let doc = normalized.document();
            if record_ref(doc.reference())? != d.reference || doc.canonical_bytes() != d.bytes {
                return Err(ServiceError::IntegrityFailure);
            }
            typed.insert(id.clone(), doc.clone());
            progress = true;
        }
        if !progress {
            return Err(reject("DESCRIPTOR_DEPENDENCY_MISSING"));
        }
    }
    let refs = core::candidate_dependency_refs(raw).map_err(transport_error)?;
    core::normalize_retained_definition(raw, &dependencies(&refs, &typed)?).map_err(transport_error)
}
fn dependencies(
    seeds: &[core::DefinitionRef],
    typed: &std::collections::BTreeMap<String, core::DefinitionDocument>,
) -> Result<Vec<core::DefinitionDocument>> {
    let mut todo = seeds.to_vec();
    let mut ids = std::collections::BTreeSet::new();
    let mut docs = vec![];
    while let Some(r) = todo.pop() {
        if !ids.insert(r.id().to_owned()) {
            continue;
        }
        let d = typed
            .get(r.id())
            .filter(|d| d.reference() == &r)
            .ok_or_else(|| reject("DESCRIPTOR_DEPENDENCY_MISSING"))?;
        todo.extend(core::candidate_dependency_refs(d.canonical_bytes()).map_err(transport_error)?);
        docs.push(d.clone());
    }
    Ok(docs)
}
