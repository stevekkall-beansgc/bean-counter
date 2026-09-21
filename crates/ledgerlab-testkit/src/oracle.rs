use crate::failpoints::{Boundary, Edge};
use crate::history::Snapshot;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessError(pub String);
impl std::fmt::Display for HarnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for HarnessError {}
pub type Result<T> = std::result::Result<T, HarnessError>;

pub(crate) fn check(ok: bool, message: impl Into<String>) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(HarnessError(message.into()))
    }
}

/// Audited frozen expectations. Loading fails closed on any fixture disagreement.
#[derive(Clone, Debug)]
pub struct FixtureOracle {
    pub root: PathBuf,
    pub pre_state: Vec<u8>,
    pub post_state: Vec<u8>,
    pub operational: Vec<u8>,
    pub receipt: Vec<u8>,
    pub seed_journal: Vec<Vec<u8>>,
    pub post_journal: Vec<Vec<u8>>,
    pub seed_indexes: Vec<Vec<u8>>,
    pub post_indexes: Vec<Vec<u8>>,
    /// Logical table -> (seed row count, accepted row delta).
    pub counts: BTreeMap<String, (usize, usize)>,
    pub inputs: BTreeMap<String, Vec<u8>>,
    pub write_boundaries: Vec<Boundary>,
    pub intention_id: String,
    pub payload: Vec<u8>,
    pub payload_digest: String,
}

impl FixtureOracle {
    pub fn workspace() -> Result<Self> {
        Self::load(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
    }

    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        let output = Command::new("python3")
            .arg("-B")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("oracle/oracle.py"))
            .arg("bundle")
            .arg(root)
            .output()
            .map_err(|e| HarnessError(format!("start independent Python oracle: {e}")))?;
        check(
            output.status.success(),
            format!(
                "fixture oracle failed; stop integration: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        )?;
        let text = String::from_utf8(output.stdout).map_err(|e| HarnessError(e.to_string()))?;
        let mut blobs = BTreeMap::new();
        let mut seed_journal = Vec::new();
        let mut post_journal = Vec::new();
        let mut seed_indexes = Vec::new();
        let mut post_indexes = Vec::new();
        let mut counts = BTreeMap::new();
        let mut write_boundaries = Vec::new();
        for line in text.lines() {
            let fields: Vec<_> = line.split('\t').collect();
            match fields.as_slice() {
                ["count", table, seed, delta] => {
                    let number =
                        |s: &str| s.parse::<usize>().map_err(|e| HarnessError(e.to_string()));
                    check(
                        counts
                            .insert((*table).to_owned(), (number(seed)?, number(delta)?))
                            .is_none(),
                        "duplicate table in oracle",
                    )?;
                }
                ["write", name, count] => {
                    let count = count
                        .parse::<usize>()
                        .map_err(|e| HarnessError(e.to_string()))?;
                    for item in 0..count {
                        for edge in [Edge::Before, Edge::After] {
                            write_boundaries.push(Boundary::Write {
                                name: (*name).into(),
                                item,
                                edge,
                            });
                        }
                    }
                }
                [key, hex] => {
                    let value = decode_hex(hex)?;
                    match *key {
                        "seed_row" => seed_journal.push(value),
                        "post_row" => post_journal.push(value),
                        "seed_index" => seed_indexes.push(value),
                        "post_index" => post_indexes.push(value),
                        _ => {
                            check(
                                blobs.insert((*key).to_owned(), value).is_none(),
                                "duplicate oracle field",
                            )?;
                        }
                    }
                }
                _ => return Err(HarnessError("invalid oracle protocol".into())),
            }
        }
        let mut take = |key: &str| {
            blobs
                .remove(key)
                .ok_or_else(|| HarnessError(format!("missing oracle field {key}")))
        };
        let pre_state = take("pre_state")?;
        let post_state = take("post_state")?;
        let operational = take("operational")?;
        let receipt = take("receipt")?;
        let intention_id =
            String::from_utf8(take("intention_id")?).map_err(|e| HarnessError(e.to_string()))?;
        let payload = take("payload")?;
        let payload_digest =
            String::from_utf8(take("payload_digest")?).map_err(|e| HarnessError(e.to_string()))?;
        check(
            seed_journal.len() == 10 && post_journal.len() == 35,
            "journal cardinality",
        )?;
        check(
            write_boundaries.len() == 54,
            "expected 27 writes and 54 boundaries",
        )?;
        Ok(Self {
            root: root.to_owned(),
            pre_state,
            post_state,
            operational,
            receipt,
            seed_journal,
            post_journal,
            seed_indexes,
            post_indexes,
            counts,
            inputs: blobs,
            write_boundaries,
            intention_id,
            payload,
            payload_digest,
        })
    }

    pub fn input(&self, name: &str) -> Result<&[u8]> {
        self.inputs
            .get(name)
            .map(Vec::as_slice)
            .ok_or_else(|| HarnessError(format!("unknown fixture input {name}")))
    }

    /// Validate a failed completion using the frozen completion encoding and
    /// independently recomputed identities/membership, never a pricing call.
    pub fn verify_zero_journal(&self, journal: &[Vec<u8>], indexes: &[Vec<u8>]) -> Result<Vec<u8>> {
        let mut child = Command::new("python3")
            .arg("-B")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("oracle/oracle.py"))
            .arg("verify-zero")
            .arg(&self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| HarnessError(e.to_string()))?;
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| HarnessError("oracle stdin unavailable".into()))?;
        for row in journal {
            input
                .write_all(row)
                .and_then(|()| input.write_all(b"\n"))
                .map_err(|e| HarnessError(e.to_string()))?;
        }
        drop(input);
        let output = child
            .wait_with_output()
            .map_err(|e| HarnessError(e.to_string()))?;
        check(
            output.status.success(),
            format!("zero oracle: {}", String::from_utf8_lossy(&output.stderr)),
        )?;
        let text = String::from_utf8(output.stdout).map_err(|e| HarnessError(e.to_string()))?;
        let mut receipt = None;
        let mut expected_indexes = Vec::new();
        for line in text.lines() {
            let (kind, hex) = line
                .split_once('\t')
                .ok_or_else(|| HarnessError("zero oracle framing".into()))?;
            match kind {
                "receipt" => {
                    check(
                        receipt.replace(decode_hex(hex)?).is_none(),
                        "duplicate zero receipt",
                    )?;
                }
                "index" => expected_indexes.push(decode_hex(hex)?),
                _ => return Err(HarnessError("unknown zero oracle field".into())),
            }
        }
        check(
            indexes == expected_indexes,
            "zero stored indexed columns differ",
        )?;
        receipt.ok_or_else(|| HarnessError("missing zero receipt".into()))
    }

    pub fn assert_seed(&self, snapshot: &Snapshot) -> Result<()> {
        snapshot.validate_inventory()?;
        check(
            snapshot.indexes == self.seed_indexes,
            "seed indexed columns differ",
        )?;
        check(
            snapshot.journal == self.seed_journal,
            "seed journal differs",
        )?;
        check(
            snapshot.state == self.pre_state,
            "seed control state differs",
        )?;
        check(
            snapshot.operational.is_none() && snapshot.aliases.is_empty(),
            "unexpected seeded operational/alias state",
        )?;
        for (table, (count, _)) in &self.counts {
            check(
                snapshot.row_count(table)? == *count,
                format!("seed count {table}"),
            )?;
        }
        Ok(())
    }

    pub fn assert_accepted(&self, before: &Snapshot, after: &Snapshot) -> Result<()> {
        self.assert_seed(before)?;
        check(
            after.indexes == self.post_indexes,
            "accepted indexed columns differ from canonical record projections",
        )?;
        check(
            after.journal == self.post_journal,
            "accepted canonical journal differs (bytes, IDs, rows or order)",
        )?;
        check(
            after.state == self.post_state,
            "accepted control state differs",
        )?;
        check(
            after.operational.as_ref() == Some(&self.operational),
            "accepted operational state differs",
        )?;
        check(after.aliases.is_empty(), "fresh accept created aliases")?;
        let deltas = self
            .counts
            .iter()
            .map(|(k, (_, v))| (k.clone(), *v))
            .collect();
        before.assert_delta(after, &deltas, &["chains"])
    }
}

fn decode_hex(text: &str) -> Result<Vec<u8>> {
    check(
        text.len().is_multiple_of(2) && text.is_ascii(),
        "invalid hex framing",
    )?;
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|e| HarnessError(e.to_string())))
        .collect()
}
