use super::{prefixed, slug, text, Scope};
use crate::canonical::{self, CanonicalBytes, Domain};
use crate::money::Decimal;
use crate::wire::{Completion, EventDto, EventKind, EventRef, Relation};
use crate::{Error, Result};
use serde_json::{json, Value};

#[derive(Clone, Debug)]
pub struct Candidate {
    scope: Scope,
    source: String,
    dto: EventDto,
    ingress: CanonicalBytes,
    ingress_hash: String,
    event_id: String,
}
#[derive(Clone, Debug)]
pub struct Event {
    candidate: Candidate,
    dto: EventDto,
    bytes: CanonicalBytes,
    content_hash: String,
}

pub fn normalize(bytes: &[u8], scope: Scope, default_source: &str) -> Result<Candidate> {
    let mut dto: EventDto = canonical::dto(canonical::parse(bytes)?)?;
    if dto.schema != "ledger-event/1" {
        return Err(Error::new("SCHEMA", "event schema"));
    }
    text(&dto.id, 128)?;
    text(&dto.customer, 128)?;
    let source = dto.source.as_deref().unwrap_or(default_source).to_string();
    validate_source(&source)?;
    for s in [
        &dto.operation_id,
        &dto.chain,
        &dto.binding_id,
        &dto.invocation_id,
        &dto.claim_id,
    ]
    .into_iter()
    .flatten()
    {
        text(s, 128)?;
    }
    dto.operation_id.get_or_insert_with(|| dto.id.clone());
    if let Some(id) = &dto.corrects {
        prefixed(id, "ev_")?;
    }
    if let Some(s) = &dto.unit {
        slug(s)?;
    }
    if let Some(s) = &dto.reason {
        text(s, 256)?;
    }
    if dto.extensions.len() > 16
        || CanonicalBytes::from_value(&dto.extensions)?
            .as_slice()
            .len()
            > 4096
        || dto
            .extensions
            .values()
            .any(|v| !matches!(v, Value::String(_) | Value::Bool(_) | Value::Number(_)))
    {
        return Err(Error::new("LIMIT", "extension scalar/count/bytes"));
    }
    if let Some(child) = &dto.child {
        validate_ref(child)?;
    }
    if let Some(links) = &mut dto.links {
        if links.len() > 32 {
            return Err(Error::new("LIMIT", "links"));
        }
        for link in links.iter() {
            if link.schema != "ledger-link/1" {
                return Err(Error::new("SCHEMA", "link schema"));
            }
            validate_ref(&link.from)?;
            if link.from.source == source && link.from.id == dto.id {
                return Err(Error::new("LINK_SELF", "self reference"));
            }
        }
        canonical::sort_set(links)?;
    }
    if let Some(evidence) = &mut dto.evidence {
        if evidence.len() > 16 {
            return Err(Error::new("LIMIT", "evidence"));
        }
        for id in evidence.iter() {
            prefixed(id, "doc_")?;
        }
        canonical::sort_set(evidence)?;
    }
    if let Some(targets) = &mut dto.targets {
        if targets.is_empty() || targets.len() > 32 {
            return Err(Error::new("LIMIT", "reversal targets"));
        }
        for id in targets.iter() {
            prefixed(id, "ev_")?;
        }
        canonical::sort_set(targets)?;
    }
    if dto.kind.is_work() {
        if dto.claim_id.is_some()
            || dto.child.is_some()
            || dto.targets.is_some()
            || dto.reason.is_some()
        {
            return Err(Error::new("SCHEMA", "fields forbidden for completion"));
        }
        let status = *dto.status.get_or_insert(Completion::Succeeded);
        let quantity = dto.quantity.get_or_insert(Decimal::parse("1")?);
        if status == Completion::Succeeded && quantity.is_zero() {
            return Err(Error::new(
                "QUANTITY",
                "successful work requires positive quantity",
            ));
        }
        dto.unit.get_or_insert_with(|| "call".into());
        dto.links.get_or_insert_with(Vec::new);
        dto.evidence.get_or_insert_with(Vec::new);
    } else {
        if dto.status.is_some()
            || dto.quantity.is_some()
            || dto.unit.is_some()
            || dto.corrects.is_some()
        {
            return Err(Error::new("SCHEMA", "completion fields on non-work event"));
        }
        match dto.kind {
            EventKind::Acquired => {
                if dto.claim_id.is_none()
                    || dto.occurred_at.is_none()
                    || dto.links.as_ref().is_none_or(Vec::is_empty)
                    || dto.evidence.as_ref().is_none_or(Vec::is_empty)
                    || dto.child.is_some()
                    || dto.targets.is_some()
                    || dto.reason.is_some()
                {
                    return Err(Error::new("SCHEMA", "acquisition fields"));
                }
            }
            EventKind::LinkAsserted => {
                if dto.child.is_none()
                    || dto.links.as_ref().is_none_or(|v| v.len() != 1)
                    || dto.claim_id.is_some()
                    || dto.targets.is_some()
                    || dto.reason.is_some()
                {
                    return Err(Error::new("SCHEMA", "link assertion fields"));
                }
            }
            EventKind::Reversal => {
                if dto.targets.is_none()
                    || dto.reason.is_none()
                    || dto.evidence.as_ref().is_none_or(|v| v.len() != 1)
                    || dto.claim_id.is_some()
                    || dto.child.is_some()
                    || dto.links.is_some()
                    || dto.binding_id.is_some()
                    || dto.invocation_id.is_some()
                {
                    return Err(Error::new("SCHEMA", "reversal fields"));
                }
            }
            _ => unreachable!(),
        }
    }
    validate_link_cardinality(&dto)?;
    let ingress = CanonicalBytes::from_value(&dto)?;
    if ingress.as_slice().len() > canonical::CANDIDATE_LIMIT {
        return Err(Error::new("LIMIT", "normalized ingress"));
    }
    let ingress_hash = canonical::digest(Domain::Ingress, &dto)?;
    let event_id = canonical::identity(
        Domain::Event,
        &json!([scope.tenant(), scope.environment(), source, dto.id]),
    )?;
    Ok(Candidate {
        scope,
        source,
        dto,
        ingress,
        ingress_hash,
        event_id,
    })
}
impl Candidate {
    pub fn ingress_bytes(&self) -> &CanonicalBytes {
        &self.ingress
    }
    pub fn ingress_hash(&self) -> &str {
        &self.ingress_hash
    }
    pub fn event_id(&self) -> &str {
        &self.event_id
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn external_id(&self) -> &str {
        &self.dto.id
    }
    pub fn operation_id(&self) -> &str {
        self.dto
            .operation_id
            .as_deref()
            .expect("normalized operation")
    }
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
    pub fn requested_chain(&self) -> Option<&str> {
        self.dto.chain.as_deref()
    }
    /// The coordinator supplies a common parent chain after its topology checks.
    pub fn resolve(self, parent_chain: Option<&str>) -> Result<Event> {
        let mut dto = self.dto.clone();
        dto.source = Some(self.source.clone());
        if let Some(parent) = parent_chain {
            text(parent, 128)?;
            if dto.chain.as_deref().is_some_and(|c| c != parent) {
                return Err(Error::new("CHAIN_MISMATCH", "explicit and parent chain"));
            }
        }
        let chain = match (&dto.chain, parent_chain) {
            (Some(c), _) => c.clone(),
            (None, Some(c)) => c.into(),
            (None, None) => {
                if dto.links.as_ref().is_some_and(|l| !l.is_empty())
                    || dto.kind == EventKind::LinkAsserted
                {
                    return Err(Error::new("UNRESOLVED_INPUT", "parent chain required"));
                }
                format!(
                    "auto-{}",
                    canonical::hash(
                        Domain::Chain,
                        &json!([self.scope, self.source, self.operation_id()])
                    )?
                )
            }
        };
        dto.chain = Some(chain);
        let bytes = CanonicalBytes::from_value(&dto)?;
        if bytes.as_slice().len() > canonical::CANDIDATE_LIMIT {
            return Err(Error::new("LIMIT", "canonical event"));
        }
        let content_hash = canonical::digest(Domain::EventContent, &dto)?;
        Ok(Event {
            candidate: self,
            dto,
            bytes,
            content_hash,
        })
    }
}
impl Event {
    pub fn candidate(&self) -> &Candidate {
        &self.candidate
    }
    pub fn dto(&self) -> &EventDto {
        &self.dto
    }
    pub fn id(&self) -> &str {
        self.candidate.event_id()
    }
    pub fn scope(&self) -> &Scope {
        self.candidate.scope()
    }
    pub fn source(&self) -> &str {
        self.candidate.source()
    }
    pub fn chain(&self) -> &str {
        self.dto.chain.as_deref().expect("resolved chain")
    }
    pub fn bytes(&self) -> &CanonicalBytes {
        &self.bytes
    }
    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }
    pub fn completion_claim_id(&self) -> Result<String> {
        if !self.dto.kind.is_work() {
            return Err(Error::new(
                "UNSUPPORTED_SLICE",
                "only completion claim encoding is frozen",
            ));
        }
        canonical::identity(
            Domain::Claim,
            &json!([
                self.scope(),
                self.source(),
                self.candidate.operation_id(),
                "completion",
                "completion"
            ]),
        )
    }
    /// Already resolved relation/child/predecessor triples are supplied as immutable data.
    pub fn completion_facts(&self, resolved_links: &[(Relation, String, String)]) -> Result<Value> {
        self.completion_claim_id()?;
        if resolved_links.len() != self.dto.links.as_ref().map_or(0, Vec::len) {
            return Err(Error::new(
                "UNRESOLVED_INPUT",
                "resolved link projection count",
            ));
        }
        for (_, child, parent) in resolved_links {
            prefixed(child, "ev_")?;
            prefixed(parent, "ev_")?;
            if child != self.id() || parent == child {
                return Err(Error::new("LINK_SELF", "resolved endpoints"));
            }
        }
        let mut links = resolved_links.to_vec();
        canonical::sort_set(&mut links)?;
        let mut facts = json!({"schema":"ledger-claim-facts/1","type":self.dto.kind,"chain":self.chain(),"customer":self.dto.customer,"status":self.dto.status,"quantity":self.dto.quantity,"unit":self.dto.unit,"links":links,"evidence":self.dto.evidence});
        let object = facts.as_object_mut().expect("object");
        for (name, value) in [
            ("binding_id", self.dto.binding_id.as_ref()),
            ("invocation_id", self.dto.invocation_id.as_ref()),
            ("corrects", self.dto.corrects.as_ref()),
        ] {
            if let Some(value) = value {
                object.insert(name.into(), json!(value));
            }
        }
        if let Some(t) = &self.dto.occurred_at {
            object.insert("occurred_at".into(), json!(t));
        }
        Ok(facts)
    }
}
pub(crate) fn validate_source(s: &str) -> Result<()> {
    text(s, 256)?;
    let scheme = s
        .split_once(':')
        .map(|(scheme, _)| scheme)
        .ok_or_else(|| Error::new("SOURCE", "absolute source URI required"))?;
    if scheme.is_empty()
        || !scheme.as_bytes()[0].is_ascii_alphabetic()
        || !scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+.-".contains(&b))
        || s.chars().any(char::is_whitespace)
    {
        return Err(Error::new("SOURCE", "source URI"));
    }
    Ok(())
}
fn validate_ref(r: &EventRef) -> Result<()> {
    validate_source(&r.source)?;
    text(&r.id, 128)
}
fn validate_link_cardinality(dto: &EventDto) -> Result<()> {
    let links = dto.links.as_deref().unwrap_or(&[]);
    if dto.kind == EventKind::LinkAsserted {
        return Ok(());
    }
    for relation in [
        Relation::GeneratedFrom,
        Relation::OptimizedFrom,
        Relation::PublishedAs,
        Relation::AttributedTo,
        Relation::ConsumesService,
    ] {
        let n = links.iter().filter(|l| l.relation == relation).count();
        let max = match (dto.kind, relation) {
            (EventKind::Generated, Relation::GeneratedFrom) => 8,
            (EventKind::Optimized, Relation::OptimizedFrom)
            | (EventKind::Published, Relation::PublishedAs)
            | (EventKind::Acquired, Relation::AttributedTo) => 1,
            (
                EventKind::Generated | EventKind::Optimized | EventKind::Published,
                Relation::ConsumesService,
            ) => 8,
            _ => 0,
        };
        if n > max {
            return Err(Error::new(
                "LINK_CARDINALITY",
                "relation not allowed or too many",
            ));
        }
        let required = matches!(
            (dto.kind, relation),
            (EventKind::Optimized, Relation::OptimizedFrom)
                | (EventKind::Published, Relation::PublishedAs)
        ) && dto.status == Some(Completion::Succeeded)
            || dto.kind == EventKind::Acquired && relation == Relation::AttributedTo;
        if required && n != 1 {
            return Err(Error::new(
                "LINK_CARDINALITY",
                "required predecessor missing",
            ));
        }
    }
    Ok(())
}
