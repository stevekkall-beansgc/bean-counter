//! Wire DTOs are not validated domain values. Use `domain::normalize` at ingress.
use crate::domain::Timestamp;
use crate::money::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EventKind {
    #[serde(rename = "content.generated")]
    Generated,
    #[serde(rename = "tool.optimized")]
    Optimized,
    #[serde(rename = "content.published")]
    Published,
    #[serde(rename = "tool.completed")]
    ToolCompleted,
    #[serde(rename = "outcome.acquired")]
    Acquired,
    #[serde(rename = "link.asserted")]
    LinkAsserted,
    #[serde(rename = "economic.reversal")]
    Reversal,
}
impl EventKind {
    pub fn is_work(self) -> bool {
        matches!(
            self,
            Self::Generated | Self::Optimized | Self::Published | Self::ToolCompleted
        )
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Completion {
    Succeeded,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    GeneratedFrom,
    OptimizedFrom,
    PublishedAs,
    AttributedTo,
    ConsumesService,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventRef {
    pub source: String,
    pub id: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypedLink {
    #[serde(default = "link_schema")]
    pub schema: String,
    pub relation: Relation,
    pub from: EventRef,
}
fn link_schema() -> String {
    "ledger-link/1".into()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventDto {
    pub schema: String,
    pub id: String,
    #[serde(rename = "type")]
    pub kind: EventKind,
    pub customer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Completion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantity: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<TypedLink>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child: Option<EventRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub targets: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corrects: Option<String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, Value>,
}
