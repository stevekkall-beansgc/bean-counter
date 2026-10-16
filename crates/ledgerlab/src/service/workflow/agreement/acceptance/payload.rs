use super::super::*;
use ledgerlab_core::policy::workflow::{
    AcceptanceTerms, AgreementRef, AssentRecord, FactSchema, Product, RoleAssignment,
    SlotBindingTerms, SlotCatalog, SlotInstantiation, SlotTemplate,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    schema: String,
    agreement_id: String,
    expected_revision: String,
    publication_ref: RecordRef,
    slot_catalog_ref: RecordRef,
    slot_instantiation_ref: RecordRef,
    binding_inputs: Value,
    assent_refs: Vec<RecordRef>,
    authority_refs: Vec<RecordRef>,
    documents: Vec<Value>,
}
/// Validated input assertions. No origin, grant, assent or CAS capability is
/// created by this codec; even supplied retained candidates need host proof.
pub(in crate::service::workflow::agreement) struct AcceptInput {
    input: Payload,
    terms: AcceptanceTerms,
    context: FactSchema,
}
impl AcceptInput {
    pub(in crate::service::workflow::agreement) fn parse(
        raw: &[u8],
        scope: &[String; 2],
        context: &FactSchema,
    ) -> Result<Self> {
        let value = canonical::parse_bounded(raw, 262144).map_err(|_| reject("INVALID_SHAPE"))?;
        require(no_null(&value), "INVALID_SHAPE")?;
        let mut input: Payload =
            serde_json::from_value(value).map_err(|_| reject("INVALID_SHAPE"))?;
        require(
            matches!(
                input.schema.as_str(),
                "ledger-workflow-agreement-accept/2"
                    | "ledger-workflow-agreement-accept/3"
                    | "ledger-workflow-agreement-accept/4"
            ) && input.documents.len() <= 128,
            "INVALID_SHAPE",
        )?;
        input.expected_revision = counter(&input.expected_revision)?;
        validate_ref(&input.publication_ref, "workflow-publication")?;
        validate_ref(&input.slot_catalog_ref, "workflow-slot-catalog")?;
        validate_ref(&input.slot_instantiation_ref, "workflow-slot-instantiation")?;
        normalize_refs(&mut input.assent_refs, "workflow-assent", 1, 16)?;
        // The initial retail-unit implementation has only enrolled source grants.
        normalize_refs(&mut input.authority_refs, "workflow-source-grant", 1, 32)?;
        let mut projected = input.binding_inputs.clone();
        let object = projected
            .as_object_mut()
            .ok_or_else(|| reject("INVALID_SHAPE"))?;
        let mut assents: Vec<RecordRef> = serde_json::from_value(
            object
                .remove("assent_refs")
                .ok_or_else(|| reject("INVALID_SHAPE"))?,
        )
        .map_err(|_| reject("INVALID_SHAPE"))?;
        let mut authorities: Vec<RecordRef> = serde_json::from_value(
            object
                .remove("authority_refs")
                .ok_or_else(|| reject("INVALID_SHAPE"))?,
        )
        .map_err(|_| reject("INVALID_SHAPE"))?;
        normalize_refs(&mut assents, "workflow-assent", 1, 16)?;
        normalize_refs(&mut authorities, "workflow-source-grant", 1, 32)?;
        require(
            assents == input.assent_refs && authorities == input.authority_refs,
            "INVALID_REFERENCE",
        )?;
        let body = json!({"schema":"ledger-workflow-acceptance-terms/1","scope":scope,"publication_ref":input.publication_ref,"agreement_id":input.agreement_id,"customer":projected["customer"],"version":projected["version"],"binding_inputs":projected});
        let terms =
            AcceptanceTerms::parse(&records::bytes(&body)?, context).map_err(typed_error)?;
        let normalized = &terms.canonical_value()["binding_inputs"];
        let next_revision = input
            .expected_revision
            .parse::<u64>()
            .ok()
            .and_then(|revision| revision.checked_add(1))
            .ok_or_else(|| reject("INVALID_SCALAR"))?;
        let next_revision = next_revision.to_string();
        require(
            normalized["version"].as_str() == Some(next_revision.as_str()),
            "INVALID_SHAPE",
        )?;
        require(
            normalized["slot_catalog_ref"] == json!(input.slot_catalog_ref)
                && normalized["slot_instantiation_ref"] == json!(input.slot_instantiation_ref),
            "INVALID_REFERENCE",
        )?;
        input.binding_inputs = normalized.clone();
        input.binding_inputs["assent_refs"] = json!(assents);
        input.binding_inputs["authority_refs"] = json!(authorities);
        Ok(Self {
            input,
            terms,
            context: context.clone(),
        })
    }
    pub(in crate::service::workflow::agreement) fn terms(&self) -> &AcceptanceTerms {
        &self.terms
    }
    pub(in crate::service::workflow::agreement) fn expected_revision(&self) -> &str {
        &self.input.expected_revision
    }
    pub(in crate::service::workflow::agreement) fn agreement_id(&self) -> &str {
        &self.input.agreement_id
    }
    pub(in crate::service::workflow::agreement) fn customer(&self) -> &str {
        self.input.binding_inputs["customer"]
            .as_str()
            .expect("typed terms")
    }
    /// Exact direct private refs plus derived terms, never arbitrary JSON scanning.
    pub(in crate::service::workflow::agreement) fn private_refs(&self) -> Result<Vec<RecordRef>> {
        let mut refs = vec![
            self.input.slot_catalog_ref.clone(),
            self.input.slot_instantiation_ref.clone(),
            from_core(self.terms.reference()),
        ];
        refs.extend(self.input.assent_refs.clone());
        for value in self.input.binding_inputs["roles_refs"]
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            let r: RecordRef = serde_json::from_value(value.clone())
                .map_err(|_| ServiceError::IntegrityFailure)?;
            validate_ref(&r, "workflow-roles")?;
            refs.push(r);
        }
        for value in self.input.binding_inputs["bindings"]
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            let r: RecordRef = serde_json::from_value(value["binding_ref"].clone())
                .map_err(|_| ServiceError::IntegrityFailure)?;
            validate_ref(&r, "workflow-slot-terms")?;
            refs.push(r);
        }
        let mut map = BTreeMap::new();
        for r in refs {
            map.insert(records::bytes(&r)?, r);
        }
        let mut refs: Vec<_> = map.into_values().collect();
        records::sort_refs(&mut refs)?;
        Ok(refs)
    }
    /// Supplied candidates are scoped byte assertions until the coordinator
    /// verifies their saved originating certificates. Inline docs remain tentative.
    pub(in crate::service::workflow::agreement) fn normalize_private_documents(
        &mut self,
        template: &SlotTemplate,
        product: &Product,
        retained: &[Document],
    ) -> Result<Vec<Document>> {
        self.terms.validate_product(product).map_err(typed_error)?;
        require(
            retained.len() <= 128
                && retained.iter().map(|d| d.bytes.len()).sum::<usize>() <= 8 * 1024 * 1024,
            "REFERENCE_LIMIT",
        )?;
        let mut values = BTreeMap::new();
        for d in retained {
            require(d.bytes.len() <= 262144, "REFERENCE_LIMIT")?;
            let v = canonical::parse_bounded(&d.bytes, 262144)
                .map_err(|_| ServiceError::IntegrityFailure)?;
            require(
                records::source_document(&d.reference.kind, v.clone())?.reference == d.reference
                    && records::bytes(&v)? == d.bytes,
                "INVALID_REFERENCE",
            )?;
            require(private_kind(&v).is_some(), "INVALID_REFERENCE")?;
            values.insert(d.reference.id.clone(), (v, false));
        }
        let mut inline_ids = BTreeSet::new();
        let mut normalized = vec![];
        // First pass admits only closed independently parseable families.
        for v in &self.input.documents {
            require(private_kind(v).is_some(), "INVALID_SHAPE")?;
            if v["schema"] == "ledger-workflow-slot-instantiation/1"
                || v["schema"] == "ledger-workflow-assent/1"
            {
                continue;
            }
            let d = normalize_leaf(v, &self.terms, &self.context)?;
            require(inline_ids.insert(d.reference.id.clone()), "INVALID_SHAPE")?;
            let body = canonical::parse(&d.bytes).map_err(|_| ServiceError::IntegrityFailure)?;
            values.insert(d.reference.id.clone(), (body, true));
            normalized.push(d);
        }
        let catalog_value = &values
            .get(&self.input.slot_catalog_ref.id)
            .ok_or_else(|| reject("INVALID_REFERENCE"))?
            .0;
        let catalog = SlotCatalog::parse(&records::bytes(catalog_value)?).map_err(typed_error)?;
        require(
            catalog.agreement_id() == self.agreement_id() && catalog.customer() == self.customer(),
            "INVALID_REFERENCE",
        )?;
        for v in &self.input.documents {
            let d = match v["schema"].as_str() {
                Some("ledger-workflow-slot-instantiation/1") => {
                    let typed = SlotInstantiation::parse(&records::bytes(v)?, template, &catalog)
                        .map_err(typed_error)?;
                    records::source_document(
                        "workflow-slot-instantiation",
                        typed.canonical_value().clone(),
                    )?
                }
                Some("ledger-workflow-assent/1") => {
                    let typed = AssentRecord::parse(&records::bytes(v)?, &self.terms)
                        .map_err(typed_error)?;
                    records::source_document("workflow-assent", typed.canonical_value().clone())?
                }
                _ => continue,
            };
            require(inline_ids.insert(d.reference.id.clone()), "INVALID_SHAPE")?;
            values.insert(
                d.reference.id.clone(),
                (
                    canonical::parse(&d.bytes).map_err(|_| ServiceError::IntegrityFailure)?,
                    true,
                ),
            );
            normalized.push(d);
        }
        let required = self.private_refs()?;
        require(
            inline_ids
                .iter()
                .all(|id| required.iter().any(|r| &r.id == id)),
            "INVALID_REFERENCE",
        )?;
        let mut closure = vec![];
        for r in &required {
            let (v, _) = values
                .get(&r.id)
                .ok_or_else(|| reject("INVALID_REFERENCE"))?;
            let d = match r.kind.as_str() {
                "workflow-slot-instantiation" => {
                    let typed = SlotInstantiation::parse(&records::bytes(v)?, template, &catalog)
                        .map_err(typed_error)?;
                    records::source_document(&r.kind, typed.canonical_value().clone())?
                }
                "workflow-assent" => {
                    let typed = AssentRecord::parse(&records::bytes(v)?, &self.terms)
                        .map_err(typed_error)?;
                    records::source_document(&r.kind, typed.canonical_value().clone())?
                }
                _ => normalize_leaf(v, &self.terms, &self.context)?,
            };
            require(d.reference == *r, "INVALID_REFERENCE")?;
            let body = canonical::parse(&d.bytes).map_err(|_| ServiceError::IntegrityFailure)?;
            require(
                body["agreement_id"] == self.agreement_id() && body["customer"] == self.customer(),
                "INVALID_REFERENCE",
            )?;
            if r.kind == "workflow-roles"
                || r.kind == "workflow-slot-terms"
                || r.kind == "workflow-acceptance-terms"
                || r.kind == "workflow-assent"
            {
                require(
                    body["scope"] == self.terms.canonical_value()["scope"],
                    "INVALID_REFERENCE",
                )?;
            }
            if r.kind == "workflow-slot-terms" {
                require(
                    match body["schema"].as_str() {
                        Some("ledger-workflow-slot-binding-terms/2") => matches!(
                            self.input.schema.as_str(),
                            "ledger-workflow-agreement-accept/3"
                                | "ledger-workflow-agreement-accept/4"
                        ),
                        Some("ledger-workflow-slot-binding-terms/3") => {
                            self.input.schema == "ledger-workflow-agreement-accept/4"
                        }
                        _ => true,
                    },
                    "UNSUPPORTED_SCHEMA_PROFILE",
                )?;
                let role: RecordRef = serde_json::from_value(body["role_ref"].clone())
                    .map_err(|_| ServiceError::IntegrityFailure)?;
                require(required.contains(&role), "INVALID_REFERENCE")?;
                let role_value = &values
                    .get(&role.id)
                    .ok_or_else(|| reject("INVALID_REFERENCE"))?
                    .0;
                let role =
                    RoleAssignment::parse(&records::bytes(role_value)?).map_err(typed_error)?;
                let terms = SlotBindingTerms::parse(&d.bytes).map_err(typed_error)?;
                terms.validate_role(&role).map_err(typed_error)?;
                terms.validate_product(product).map_err(typed_error)?;
            }
            closure.push(d);
        }
        require(
            closure.iter().map(|d| d.bytes.len()).sum::<usize>() <= 8 * 1024 * 1024,
            "REFERENCE_LIMIT",
        )?;
        self.input.documents = normalized
            .iter()
            .map(|d| canonical::parse(&d.bytes).map_err(|_| ServiceError::IntegrityFailure))
            .collect::<Result<Vec<_>>>()?;
        canonical::sort_set(&mut self.input.documents).map_err(|_| reject("INVALID_SHAPE"))?;
        Ok(closure)
    }
    pub(in crate::service::workflow::agreement) fn value(&self) -> Result<Value> {
        serde_json::to_value(&self.input).map_err(|_| ServiceError::IntegrityFailure)
    }
    /// Transport-independent comparison only. The immutable Intent record must
    /// retain value(), including the normalized inline documents.
    pub(in crate::service::workflow::agreement) fn semantic_intent(&self) -> Result<Value> {
        let mut v = self.value()?;
        v.as_object_mut()
            .ok_or(ServiceError::IntegrityFailure)?
            .remove("documents");
        Ok(v)
    }
}
fn normalize_leaf(v: &Value, terms: &AcceptanceTerms, context: &FactSchema) -> Result<Document> {
    let bytes = records::bytes(v)?;
    require(bytes.len() <= 262144, "REFERENCE_LIMIT")?;
    let (kind, body) = match v["schema"].as_str() {
        Some("ledger-workflow-role-assignment/1") => (
            "workflow-roles",
            RoleAssignment::parse(&bytes)
                .map_err(typed_error)?
                .canonical_value()
                .clone(),
        ),
        Some("ledger-workflow-slot-catalog/1") => (
            "workflow-slot-catalog",
            SlotCatalog::parse(&bytes)
                .map_err(typed_error)?
                .canonical_value()
                .clone(),
        ),
        Some(
            "ledger-workflow-slot-binding-terms/1"
            | "ledger-workflow-slot-binding-terms/2"
            | "ledger-workflow-slot-binding-terms/3",
        ) => (
            "workflow-slot-terms",
            SlotBindingTerms::parse(&bytes)
                .map_err(typed_error)?
                .canonical_value()
                .clone(),
        ),
        Some("ledger-workflow-acceptance-terms/1") => {
            let normalized = AcceptanceTerms::parse(&bytes, context).map_err(typed_error)?;
            require(
                normalized.reference() == terms.reference(),
                "ASSENT_TERMS_MISMATCH",
            )?;
            (
                "workflow-acceptance-terms",
                normalized.canonical_value().clone(),
            )
        }
        _ => return Err(reject("INVALID_SHAPE")),
    };
    records::source_document(kind, body)
}
fn private_kind(v: &Value) -> Option<&'static str> {
    match v["schema"].as_str()? {
        "ledger-workflow-role-assignment/1" => Some("workflow-roles"),
        "ledger-workflow-slot-catalog/1" => Some("workflow-slot-catalog"),
        "ledger-workflow-slot-instantiation/1" => Some("workflow-slot-instantiation"),
        "ledger-workflow-slot-binding-terms/1"
        | "ledger-workflow-slot-binding-terms/2"
        | "ledger-workflow-slot-binding-terms/3" => Some("workflow-slot-terms"),
        "ledger-workflow-acceptance-terms/1" => Some("workflow-acceptance-terms"),
        "ledger-workflow-assent/1" => Some("workflow-assent"),
        _ => None,
    }
}
fn counter(s: &str) -> Result<String> {
    require(
        !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_digit()),
        "INVALID_SHAPE",
    )?;
    let n = s.trim_start_matches('0');
    let value = if n.is_empty() {
        0
    } else {
        n.parse::<i64>().map_err(|_| reject("INVALID_SCALAR"))?
    };
    Ok(value.to_string())
}
fn validate_ref(r: &RecordRef, kind: &str) -> Result<()> {
    let typed = AgreementRef::parse(&records::bytes(r)?).map_err(typed_error)?;
    require(typed.kind() == kind, "INVALID_REFERENCE")
}
fn normalize_refs(refs: &mut Vec<RecordRef>, kind: &str, min: usize, max: usize) -> Result<()> {
    require((min..=max).contains(&refs.len()), "INVALID_SHAPE")?;
    for r in refs.iter() {
        validate_ref(r, kind)?;
    }
    records::sort_refs(refs).map_err(|_| reject("INVALID_SHAPE"))
}
fn from_core(r: &AgreementRef) -> RecordRef {
    RecordRef {
        kind: r.kind().into(),
        id: r.id().into(),
        digest: r.digest().into(),
    }
}
fn no_null(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Array(a) => a.iter().all(no_null),
        Value::Object(o) => o.values().all(no_null),
        _ => true,
    }
}

#[cfg(test)]
mod tests;
