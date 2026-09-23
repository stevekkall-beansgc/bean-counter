//! Current trusted-host observation plus exact retained authority preimages.
use super::*;
use ledgerlab_core::adjudication::{self as r3, Validate};
use ledgerlab_core::{Error, Result};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
fn err(code: &'static str, detail: &str) -> Error {
    Error {
        code,
        detail: detail.into(),
    }
}
fn require(ok: bool, code: &'static str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error {
            code,
            detail: "authority source binding".into(),
        })
    }
}
pub(super) struct Sources {
    pub records: BTreeMap<String, (wire::AuthoritySource, Value)>,
    pub used: RefCell<BTreeSet<String>>,
}
impl Sources {
    pub(super) fn new(values: &[wire::AuthoritySource]) -> Result<Self> {
        require(values.len() <= 83, "AUTH_SOURCE_COUNT")?;
        let mut records = BTreeMap::new();
        let mut identities = BTreeMap::new();
        let mut total = 0usize;
        for row in values {
            row.validate()?;
            let raw = r3::proofs::decode_base64(&row.body, 16384)?;
            total += raw.len();
            require(total <= 524288, "AUTH_SOURCE_BYTES")?;
            require(
                row.bytes.value() == raw.len() as u128 && r3::raw_sha256(&raw) == row.body_hash,
                "AUTH_SOURCE_HASH",
            )?;
            let body: wire::AuthoritySourceBody = r3::parse_exact(&raw, 16384)?;
            let value = serde_json::to_value(body).map_err(|_| Error {
                code: "AUTH_SOURCE_SHAPE",
                detail: "source".into(),
            })?;
            let identity = r3::canonical_bytes(
                &json!([value["source"], value["id"], value["revision"]]),
                4096,
            )?;
            if let Some(old) = identities.insert(identity, row.body_hash.clone()) {
                require(old == row.body_hash, "AUTH_SOURCE_IDENTITY_CONFLICT")?;
            }
            require(
                records
                    .insert(row.body_hash.as_str().into(), (row.clone(), value))
                    .is_none(),
                "AUTH_SOURCE_DUPLICATE",
            )?;
        }
        Ok(Self {
            records,
            used: RefCell::new(BTreeSet::new()),
        })
    }
    pub(super) fn get(&self, digest: &Digest) -> Result<&Value> {
        self.used.borrow_mut().insert(digest.as_str().into());
        self.records
            .get(digest.as_str())
            .map(|r| &r.1)
            .ok_or_else(|| Error {
                code: "AUTH_SOURCE_MEMBERSHIP",
                detail: "exact source missing".into(),
            })
    }
    fn source(&self, reference: &Value, kind: &str, context: &Value) -> Result<&Value> {
        let h = Digest::parse(
            reference
                .as_str()
                .ok_or_else(|| err("AUTH_REFERENCE", "digest"))?,
        )?;
        let body = self.get(&h)?;
        require(
            body["kind"] == kind
                && body["scope"] == context["scope"]
                && body["target"] == context["target"],
            "AUTH_SOURCE_CONTEXT",
        )?;
        Ok(body)
    }
    fn assent(
        &self,
        reference: &Value,
        roles: &Value,
        terms: &Value,
        context: &Value,
    ) -> Result<()> {
        let body = self.source(reference, "ASSENT", context)?;
        require(
            body["roles"] == *roles
                && body["terms"] == r3::runtime::hash("authority", terms)?.as_str(),
            "ASSENT_BINDING",
        )
    }
    fn delegation(
        &self,
        roles: &Value,
        context: &Value,
        agreements: &[Value],
        exposure: u128,
        now: &str,
        finish: Option<&str>,
    ) -> Result<Option<String>> {
        if roles["bearer"] == roles["payer"] && roles.get("payer_delegation").is_none() {
            return Ok(None);
        }
        let reference = roles
            .get("payer_delegation")
            .ok_or_else(|| err("PAYER_DELEGATION", "required"))?;
        let body = self.source(reference, "DELEGATION", context)?;
        let mut plain = roles.clone();
        plain
            .as_object_mut()
            .ok_or_else(|| err("ROLES", "shape"))?
            .remove("payer_delegation");
        let maximum = body["maximum_exposure"]
            .as_str()
            .and_then(|s| s.parse::<u128>().ok())
            .ok_or_else(|| err("DELEGATION_EXPOSURE", "amount"))?;
        require(
            body["roles"] == plain
                && body["agreement_ids"]
                    .as_array()
                    .is_some_and(|all| agreements.iter().all(|a| all.contains(a)))
                && maximum >= exposure,
            "DELEGATION_BINDING",
        )?;
        require(
            body["acceptor"] == roles["payer"]
                && body["starts_at"].as_str().is_some_and(|s| s <= now)
                && body["ends_at"]
                    .as_str()
                    .is_some_and(|s| now < s && finish.is_none_or(|f| f < s)),
            "DELEGATION_WINDOW",
        )?;
        let mut terms = body.clone();
        terms
            .as_object_mut()
            .ok_or_else(|| err("DELEGATION", "shape"))?
            .remove("assent");
        require(
            body["assent"]["accepted_at"]
                .as_str()
                .is_some_and(|s| s <= now)
                && body["assent"]["terms"] == r3::runtime::hash("authority", &terms)?.as_str(),
            "DELEGATION_ASSENT",
        )?;
        Ok(Some(reference.as_str().expect("validated digest").into()))
    }
    pub(super) fn enrollment(&self, command: &ParsedCommand) -> Result<()> {
        let c = r3::runtime::command_value(command.command())?;
        if c["kind"] != "ENROLL" {
            return Ok(());
        }
        let p = &c["payload"];
        let now = c["authority"]["observed_at"]
            .as_str()
            .expect("validated time");
        let families = p["families"].as_array().expect("validated families");
        let mut exposure: BTreeMap<String, u128> = BTreeMap::new();
        for f in families {
            let mut terms = f.clone();
            terms.as_object_mut().expect("family").remove("assent");
            self.assent(&f["assent"], &f["roles"], &terms, p)?;
            let mut bound = f["ordinary_atoms"]
                .as_str()
                .expect("atoms")
                .parse::<i128>()
                .map_err(|_| err("ATOMS", "parse"))?
                .unsigned_abs();
            for amount in f["correction_atoms"].as_array().expect("corrections") {
                bound = bound.max(
                    amount
                        .as_str()
                        .expect("atoms")
                        .parse::<i128>()
                        .map_err(|_| err("ATOMS", "parse"))?
                        .unsigned_abs(),
                );
            }
            if let Some(reference) = self.delegation(
                &f["roles"],
                p,
                &[f["key"][1].clone()],
                bound,
                now,
                f["correction_by"].as_str(),
            )? {
                let total = exposure.entry(reference).or_default();
                *total = total
                    .checked_add(bound)
                    .ok_or_else(|| err("DELEGATION_EXPOSURE", "overflow"))?;
            }
        }
        let agreements: Vec<_> = families.iter().map(|f| f["key"][1].clone()).collect();
        for pool in p["pools"].as_array().expect("pools") {
            for authorization in pool["authorizations"].as_array().expect("authorizations") {
                let mut terms = pool.clone();
                let t = terms.as_object_mut().expect("pool");
                t.remove("authorizations");
                t.extend(authorization.as_object().expect("authorization").clone());
                t.remove("assent");
                self.assent(&authorization["assent"], &authorization["roles"], &terms, p)?;
                let direction = authorization["direction"].as_str().expect("direction");
                let mut bound = 0;
                if direction != "ZERO" {
                    bound = u128::MAX;
                    for key in [
                        "funding",
                        "gross",
                        if direction == "POSITIVE" {
                            "positive"
                        } else {
                            "negative"
                        },
                    ] {
                        bound = bound.min(
                            pool[key]
                                .as_str()
                                .expect("count")
                                .parse()
                                .map_err(|_| err("POOL", "amount"))?,
                        );
                    }
                }
                if let Some(reference) =
                    self.delegation(&authorization["roles"], p, &agreements, bound, now, None)?
                {
                    let total = exposure.entry(reference).or_default();
                    *total = total
                        .checked_add(bound)
                        .ok_or_else(|| err("DELEGATION_EXPOSURE", "overflow"))?;
                }
            }
        }
        for (reference, bound) in exposure {
            let body = self.get(&Digest::parse(&reference)?)?;
            require(
                body["maximum_exposure"]
                    .as_str()
                    .and_then(|s| s.parse::<u128>().ok())
                    .is_some_and(|n| n >= bound),
                "DELEGATION_EXPOSURE",
            )?;
        }
        Ok(())
    }

    pub(super) fn current(
        &self,
        command: &ParsedCommand,
        observation: &AuthorityObservation,
        target: Option<&Id>,
        access: AuthorityAccess,
    ) -> Result<()> {
        let c = r3::runtime::command_value(command.command())?;
        let a: &Value = &c["authority"];
        let h = r3::runtime::command_digest(command.command())?;
        let permission = if matches!(access, AuthorityAccess::ReadSavedResult) {
            "read"
        } else {
            match c["kind"].as_str().unwrap_or("") {
                "ENROLL" => "enroll",
                "RECEIVE" | "SUPPLEMENT" => "submit",
                "BEGIN" | "CLOSE" | "ABORT" => "close",
                "CORRECT" => "correct",
                "REPLACE_WRITER" => "replace",
                "DECIDE" if c["payload"]["path"] == "ADJUSTMENT" => "adjust",
                "DECIDE" => "decide",
                _ => "capacity",
            }
        };
        require(
            observation.command == h
                && a["command"] == h.as_str()
                && a["document"] == observation.document.as_str()
                && a["principal"] == observation.principal.as_str()
                && a["revision"] == observation.revision.value().to_string()
                && a["observed_at"] == observation.observed_at.as_str(),
            "AUTH_OBSERVATION",
        )?;
        require(
            serde_json::to_value(&observation.permission).ok() == Some(json!(permission))
                && a["permission"] == permission,
            "AUTH_PERMISSION",
        )?;
        let d = self.get(&observation.document)?;
        require(
            d["kind"] == "AUTHORIZATION"
                && d["principal"] == observation.principal.as_str()
                && d["revision"] == observation.revision.value().to_string()
                && d["scope"] == c["key"][0]
                && d["permissions"]
                    .as_array()
                    .is_some_and(|p| p.contains(&json!(permission))),
            "AUTH_SOURCE_BINDING",
        )?;
        if let Some(t) = target {
            require(d["target"] == t.as_str(), "AUTH_SOURCE_TARGET")?;
        }
        let now = observation.observed_at.as_str();
        require(
            d["starts_at"].as_str().is_some_and(|s| s <= now)
                && d["ends_at"].as_str().is_some_and(|s| now < s),
            "AUTH_SOURCE_WINDOW",
        )
    }
}
impl AuthorityObservation {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_backend(
        principal: Id,
        command: Digest,
        document: Digest,
        revision: Count,
        observed_at: ledgerlab_core::adjudication::types::Time,
        permission: wire::AuthorityPermission,
        exact_sources: Vec<wire::AuthoritySource>,
        current_heads: Vec<ObservedHead>,
    ) -> Result<Self> {
        require(
            !current_heads.is_empty() && current_heads.len() <= 83,
            "AUTH_CURRENT_HEAD",
        )?;
        Sources::new(&exact_sources)?;
        Ok(Self {
            principal,
            command,
            document,
            revision,
            observed_at,
            permission,
            exact_sources,
            current_heads,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_enrollment_assent_and_delegation_bindings() {
        let trace:Value=serde_json::from_str(include_str!("../../../../../../contracts/candidates/central-adjudication-r3-candidate1/customer-trace.json")).unwrap();
        let sources: Vec<wire::AuthoritySource> =
            serde_json::from_value(trace["initial"]["authority_sources"].clone()).unwrap();
        let command = ParsedCommand::parse(
            &r3::canonical_bytes(&trace["commands"][4], r3::COMMAND_BYTES).unwrap(),
        )
        .unwrap();
        let set = Sources::new(&sources).unwrap();
        set.enrollment(&command).unwrap();
        assert!(set.used.borrow().len() > 5);
        for field in ["target", "roles", "ordinary_atoms"] {
            let mut changed = trace["commands"][4].clone();
            if field == "target" {
                changed["payload"]["target"] = json!("wrong");
            } else if field == "roles" {
                changed["payload"]["families"][0][field]["payer"] = json!("wrong");
            } else {
                changed["payload"]["families"][0][field] = json!("1");
            }
            let c =
                ParsedCommand::parse(&r3::canonical_bytes(&changed, r3::COMMAND_BYTES).unwrap())
                    .unwrap();
            assert!(Sources::new(&sources).unwrap().enrollment(&c).is_err());
        }
        let mut bad = sources.clone();
        bad[0].body_hash = Digest::parse(&"0".repeat(64)).unwrap();
        assert!(Sources::new(&bad).is_err());
    }
}
