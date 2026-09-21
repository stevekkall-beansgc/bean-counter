//! Pure, synchronous Ledger Lab semantics. All resolved context is supplied as data.
#![forbid(unsafe_code)]

pub mod canonical;
pub mod domain;
pub mod money;
pub mod policy;
pub mod wire;

/// Version of the frozen contract family, independent of crate releases.
pub const CONTRACT_VERSION: &str = "ledgerlab-contracts/1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    pub code: &'static str,
    pub detail: String,
}
impl Error {
    pub(crate) fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
