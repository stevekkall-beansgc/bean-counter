//! Private analysis digests, never ledger identities or authority proofs.
use super::compile::require;
use crate::Result;
use sha2::{Digest, Sha256};

pub const ALGORITHM: &str = "ledgerlab-comparison/1";

/// Private analysis framing, deliberately unrelated to frozen ledger ID domains.
/// The caller supplies a deterministic descriptor; this digest is not authority.
pub fn provenance_digest(domain: &str, descriptor: &[u8]) -> Result<String> {
    require(
        matches!(domain, "snapshot" | "activity" | "report" | "candidate"),
        "COMPARISON_DOMAIN",
        "known analysis digest domain",
    )?;
    require(
        descriptor.len() <= 16 * 1024 * 1024,
        "COMPARISON_LIMIT",
        "descriptor bytes",
    )?;
    let mut hash = Sha256::new();
    hash.update(ALGORITHM.as_bytes());
    hash.update([0]);
    hash.update((domain.len() as u64).to_be_bytes());
    hash.update(domain.as_bytes());
    hash.update((descriptor.len() as u64).to_be_bytes());
    hash.update(descriptor);
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
