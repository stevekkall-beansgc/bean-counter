use super::*;
use crate::store::sqlite::BillingSnapshot;
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Change {
    schema: String,
    expected_revision: String,
    permissions: Vec<String>,
    reason: String,
}
pub(crate) struct ValidatedPermissions {
    revision: i64,
    bytes: Vec<u8>,
}
impl ValidatedPermissions {
    pub(crate) fn revision(&self) -> i64 {
        self.revision
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
fn change(raw: &[u8], initial: &Setup, revision: u64) -> Result<Change> {
    let value = canonical::parse_bounded(raw, 65536).map_err(|_| reject("BILLING_PERMISSIONS"))?;
    let c: Change = serde_json::from_value(value).map_err(|_| reject("BILLING_PERMISSIONS"))?;
    require(
        c.schema == "ledger-billing-permissions/1" && c.expected_revision == revision.to_string(),
        "STALE_REVISION",
    )?;
    require(
        !c.reason.trim().is_empty()
            && c.reason.len() <= 8192
            && c.permissions.len() <= 3
            && c.permissions
                .iter()
                .all(|p| initial.permissions.contains(p))
            && c.permissions.iter().collect::<BTreeSet<_>>().len() == c.permissions.len(),
        "BILLING_PERMISSIONS",
    )?;
    Ok(c)
}
pub(crate) fn effective(snapshot: &BillingSnapshot) -> Result<Setup> {
    let initial = Setup::parse(&snapshot.setup)?;
    let mut s = initial.clone();
    for raw in &snapshot.permissions {
        let c = change(raw, &initial, s.grant_revision)?;
        b::check(bytes(&json!(c))? == *raw)?;
        s.permissions = c.permissions;
        s.grant_revision += 1;
    }
    Ok(s)
}
pub(crate) fn prepare(
    snapshot: &BillingSnapshot,
    raw: &[u8],
) -> Result<(Value, ValidatedPermissions)> {
    let s = effective(snapshot)?;
    require(snapshot.permissions.len() < 1000, "BILLING_HISTORY_LIMIT")?;
    let c = change(raw, &Setup::parse(&snapshot.setup)?, s.grant_revision)?;
    let revision = s.grant_revision + 1;
    Ok((
        json!({"status":"permissions_updated","revision":revision.to_string(),"permissions":c.permissions}),
        ValidatedPermissions {
            revision: revision as i64,
            bytes: bytes(&json!(c))?,
        },
    ))
}
pub(super) fn verify_grants(records: &Records, snapshot: &BillingSnapshot) -> Result<()> {
    let initial = Setup::parse(&snapshot.setup)?;
    for row in records
        .rows
        .iter()
        .filter(|r| r["kind"] == "evidence" && r["body"]["purpose"] == "grant")
    {
        let body =
            canonical::parse(text(&row["body"]["utf8"])?.as_bytes()).map_err(|_| b::integrity())?;
        let revision = text(&body["revision"])?
            .parse::<usize>()
            .map_err(|_| b::integrity())?;
        b::check(revision >= 1 && revision <= snapshot.permissions.len() + 1)?;
        let rights = if revision == 1 {
            initial.permissions.clone()
        } else {
            change(
                &snapshot.permissions[revision - 2],
                &initial,
                (revision - 1) as u64,
            )?
            .permissions
        };
        b::check(
            body == json!({"scope":initial.scope,"principal":initial.operator,"source":initial.source,"permissions":rights,"revision":revision.to_string(),"active":true}),
        )?;
    }
    for authority in records
        .rows
        .iter()
        .filter(|r| r["kind"] == "authority-decision")
    {
        let a = &authority["body"];
        let grant = records
            .rows
            .iter()
            .find(|r| r["id"] == a["grant"])
            .ok_or_else(b::integrity)?;
        let body = canonical::parse(text(&grant["body"]["utf8"])?.as_bytes())
            .map_err(|_| b::integrity())?;
        let rights = array(&body["permissions"])?;
        b::check(
            a["grant_revision"] == body["revision"]
                && rights.contains(&json!("read"))
                && (a["may_submit"] != true || rights.contains(&json!("submit")))
                && (a["may_correct"] != true || rights.contains(&json!("correct"))),
        )?;
    }
    Ok(())
}
