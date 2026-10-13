//! Immutable operands resolved from an original publication certificate.
//! These checked data relationships do not authorize a source or assent.
use super::super::*;
use ledgerlab_core::policy::workflow::{FactSchema, Product, SlotTemplate};

pub(in crate::service::workflow::agreement) struct PublicationInputs {
    pub(in crate::service::workflow::agreement) product: Product,
    pub(in crate::service::workflow::agreement) template: SlotTemplate,
    pub(in crate::service::workflow::agreement) context: FactSchema,
    pub(in crate::service::workflow::agreement) policy: Value,
    pub(in crate::service::workflow::agreement) policy_ref: RecordRef,
    pub(in crate::service::workflow::agreement) product_ref: RecordRef,
    pub(in crate::service::workflow::agreement) fact_schema_refs: Vec<RecordRef>,
}
impl PublicationInputs {
    #[cfg(test)]
    pub(super) fn fixture(body: &Value, sources: &[&Document]) -> Result<Self> {
        Self::from_documents(body, sources)
    }
    pub(in crate::service::workflow::agreement) fn original(
        publication: &context::ObservedPublication,
    ) -> Result<Self> {
        let sources: Vec<_> = publication.sources().collect();
        Self::from_documents(publication.body(), &sources)
    }
    fn from_documents(body: &Value, sources: &[&Document]) -> Result<Self> {
        let lookup = |r: &RecordRef| -> Result<&Document> {
            sources
                .iter()
                .copied()
                .find(|d| d.reference == *r)
                .ok_or(ServiceError::IntegrityFailure)
        };
        let policy_ref: RecordRef = serde_json::from_value(body["root_policy_ref"].clone())
            .map_err(|_| ServiceError::IntegrityFailure)?;
        require(policy_ref.kind == "workflow-policy", "INVALID_REFERENCE")?;
        let policy = canonical::parse_bounded(&lookup(&policy_ref)?.bytes, 262144)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let product_ref: RecordRef = serde_json::from_value(policy["product_ref"].clone())
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let template_ref: RecordRef = serde_json::from_value(policy["slot_template_ref"].clone())
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let context_ref: RecordRef = serde_json::from_value(policy["context_schema_ref"].clone())
            .map_err(|_| ServiceError::IntegrityFailure)?;
        require(
            product_ref.kind == "workflow-product"
                && template_ref.kind == "workflow-slot-template"
                && context_ref.kind == "workflow-fact-schema",
            "INVALID_REFERENCE",
        )?;
        let schemas = sources
            .iter()
            .filter(|d| d.reference.kind == "workflow-fact-schema")
            .map(|d| FactSchema::parse(&d.bytes).map_err(typed_error))
            .collect::<Result<Vec<_>>>()?;
        let product =
            Product::parse(&lookup(&product_ref)?.bytes, &schemas).map_err(typed_error)?;
        let context = FactSchema::parse(&lookup(&context_ref)?.bytes).map_err(typed_error)?;
        let template =
            SlotTemplate::parse(&lookup(&template_ref)?.bytes, None).map_err(typed_error)?;
        let mut fact_schema_refs: Vec<RecordRef> =
            serde_json::from_value(product.canonical_value()["fact_schemas"].clone())
                .map_err(|_| ServiceError::IntegrityFailure)?;
        records::sort_refs(&mut fact_schema_refs)?;
        Ok(Self {
            product,
            template,
            context,
            policy,
            policy_ref,
            product_ref,
            fact_schema_refs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn vectors() -> Value {
        serde_json::from_slice(include_bytes!("../../../../../../../contracts/candidates/full-suite-v1/core/publication-independent-expected.json")).unwrap()
    }
    #[test]
    fn original_source_operands_pin_context_template_and_product_without_current_support() {
        let f = vectors();
        let vectors = f["vectors"].as_array().unwrap();
        let publication = &vectors.iter().find(|v| v["name"] == "publication").unwrap()["body"];
        // Encoding-only operands, not an authenticated publication constructor.
        // The runtime entry above accepts only a private origin-observed view.
        let docs: Vec<Document> = vectors
            .iter()
            .take(6)
            .map(|v| Document {
                reference: serde_json::from_value(v["ref"].clone()).unwrap(),
                schema: v["body"]["schema"].as_str().unwrap().into(),
                bytes: records::bytes(&v["body"]).unwrap(),
            })
            .collect();
        let refs: Vec<_> = docs.iter().collect();
        let resolved = PublicationInputs::from_documents(publication, &refs).unwrap();
        assert_eq!(
            resolved.product_ref,
            json_ref(&vectors.iter().find(|v| v["name"] == "product").unwrap()["ref"])
        );
        assert_eq!(
            resolved.policy_ref,
            json_ref(&vectors.iter().find(|v| v["name"] == "policy").unwrap()["ref"])
        );
        assert_eq!(
            resolved.context.document_id().unwrap(),
            vectors.iter().find(|v| v["name"] == "context").unwrap()["ref"]["id"]
        );
        assert_eq!(
            resolved.template.document_id().unwrap(),
            vectors.iter().find(|v| v["name"] == "template").unwrap()["ref"]["id"]
        );
        assert_eq!(resolved.fact_schema_refs.len(), 1);
        assert_eq!(resolved.policy["currency"], "USD");
        let mut missing = refs;
        missing.retain(|d| d.reference.kind != "workflow-slot-template");
        assert!(PublicationInputs::from_documents(publication, &missing).is_err());
    }
    fn json_ref(v: &Value) -> RecordRef {
        serde_json::from_value(v.clone()).unwrap()
    }
}
