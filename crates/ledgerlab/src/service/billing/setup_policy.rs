use super::*;
fn keys(v: &Value, names: &[&str]) -> Result<()> {
    require(
        v.as_object()
            .is_some_and(|o| o.len() == names.len() && names.iter().all(|k| o.contains_key(*k))),
        "BILLING_POLICY",
    )
}
fn slug(v: &Value) -> Result<()> {
    require(
        v.as_str().is_some_and(|s| {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        }),
        "BILLING_POLICY",
    )
}
fn money(v: &Value) -> Result<()> {
    keys(v, &["currency", "scale", "atoms"])?;
    require(v["currency"] == "USD" && v["scale"] == 2, "BILLING_POLICY")?;
    b::money(v).map_err(|_| reject("BILLING_POLICY"))?;
    Ok(())
}
pub(super) fn validate(s: &Setup) -> Result<()> {
    let p = &s.outcome_policy;
    keys(p, &["version", "families", "limits"])?;
    require(
        p["version"]
            .as_str()
            .is_some_and(|v| !v.is_empty() && v.len() <= 128),
        "BILLING_POLICY",
    )?;
    let families = p["families"]
        .as_array()
        .ok_or_else(|| reject("BILLING_POLICY"))?;
    let limits = p["limits"]
        .as_array()
        .ok_or_else(|| reject("BILLING_POLICY"))?;
    require(families.len() == 1 && limits.len() == 1, "BILLING_POLICY")?;
    let f = &families[0];
    keys(
        f,
        &[
            "family",
            "binding_id",
            "source",
            "correction_source",
            "evidence_required",
            "ordinary",
            "corrections",
            "codes",
            "replacement_codes",
            "allow_reversal",
        ],
    )?;
    slug(&f["family"])?;
    require(
        f["binding_id"] == s.binding
            && f["source"] == s.source
            && f["correction_source"] == s.source
            && f["evidence_required"] == true
            && f["allow_reversal"].is_boolean(),
        "BILLING_POLICY",
    )?;
    for name in ["ordinary", "corrections"] {
        let w = &f[name];
        keys(
            w,
            &["starts_at", "occurs_before", "received_by", "accepted_by"],
        )?;
        let ts = ["starts_at", "occurs_before", "received_by", "accepted_by"].map(|n| {
            w[n].as_str()
                .and_then(|t| Timestamp::parse(t).ok())
                .map(|t| t.micros())
        });
        require(
            ts.iter().all(Option::is_some) && ts[0] < ts[1] && ts[1] <= ts[2] && ts[2] <= ts[3],
            "BILLING_POLICY",
        )?;
    }
    let codes = f["codes"]
        .as_array()
        .ok_or_else(|| reject("BILLING_POLICY"))?;
    require(!codes.is_empty() && codes.len() <= 32, "BILLING_POLICY")?;
    let mut unique = BTreeSet::new();
    for c in codes {
        keys(c, &["code", "amount"])?;
        slug(&c["code"])?;
        require(unique.insert(c["code"].as_str().unwrap()), "BILLING_POLICY")?;
        keys(&c["amount"], &["kind", "money"])?;
        require(c["amount"]["kind"] == "fixed", "BILLING_POLICY")?;
        money(&c["amount"]["money"])?;
    }
    let replacements = f["replacement_codes"]
        .as_array()
        .ok_or_else(|| reject("BILLING_POLICY"))?;
    let mut replacement_set = BTreeSet::new();
    require(
        replacements.len() <= 32
            && replacements.iter().all(|v| {
                v.as_str()
                    .is_some_and(|s| unique.contains(s) && replacement_set.insert(s))
            }),
        "BILLING_POLICY",
    )?;
    keys(&limits[0], &["binding_id", "premium"])?;
    require(limits[0]["binding_id"] == s.binding, "BILLING_POLICY")?;
    money(&limits[0]["premium"])?;
    require(
        b::money(&limits[0]["premium"])?.atoms() >= 0,
        "BILLING_POLICY",
    )?;
    Ok(())
}
