//! Local sandbox convenience around the stable acceptance facade.
//! Configuration is trusted host input; event bytes never select a principal.
use crate::{
    service::{demo, inspect, store_error},
    store::{records, sqlite::SqliteStore},
    AcceptCommand, AcceptResult, Backend, Ledger, PreviewResult, PrincipalContext, ServiceError,
};
use ledgerlab_core::{
    canonical,
    domain::{Scope, Timestamp},
};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const EVENT_LIMIT: u64 = 262_144;
pub const CONFIG_LIMIT: u64 = 65_536;
#[derive(Debug)]
pub enum LocalError {
    Config(&'static str),
    Io(std::io::Error),
    Service(ServiceError),
}
impl std::fmt::Display for LocalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(s) => f.write_str(s),
            Self::Io(e) => write!(f, "local file operation failed: {e}"),
            Self::Service(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for LocalError {}
impl From<std::io::Error> for LocalError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<ServiceError> for LocalError {
    fn from(e: ServiceError) -> Self {
        Self::Service(e)
    }
}
pub type Result<T> = std::result::Result<T, LocalError>;
#[derive(Clone, Debug)]
pub enum ExplainTarget {
    Event(String),
    Chain(String),
}

pub struct LocalLedger {
    ledger: Ledger,
    principal: PrincipalContext,
    selector: String,
}
impl LocalLedger {
    /// Provision only the frozen synthetic sandbox. Refuses a nonempty destination.
    /// A failed initialization is left visible for inspection; it is never overwritten.
    pub async fn init_demo(path: &Path) -> Result<()> {
        check_path(path)?;
        if path.exists() {
            if !path.is_dir() || fs::read_dir(path)?.next().is_some() {
                return Err(LocalError::Config(
                    "init requires a new or empty directory; nothing was overwritten",
                ));
            }
        } else {
            private_dir(path)?;
        }
        let data = path.join(".ledger");
        private_dir(&data)?;
        private_dir(&path.join("examples"))?;
        demo::create(&data).await?;
        write_new(&path.join("examples/generated.json"), demo::EVENT)?;
        write_new(&path.join(".gitignore"), b".ledger/\n")?;
        let config = json!({
            "schema":"ledger/v1", "mode":"sandbox",
            "identity":{"tenant":"demo","environment":"sandbox","store_id":"store-demo-slice"},
            "storage":{"backend":"sqlite","data_dir":".ledger"},
            "auth":{"principal_id":"demo-app","source":"urn:demo:app","authority_head":"demo-source-grant-v1","binding_selector":"demo-retail-selector"},
            "dispatch":{"enabled":false,"destination":"fake"}
        });
        let mut bytes = serde_json::to_vec_pretty(&config).expect("config JSON");
        bytes.push(b'\n');
        // JSON is the dependency-free YAML 1.2 subset supported in this bounded CLI.
        write_new(&path.join("ledger.yaml"), &bytes)?;
        write_new(&path.join("README.md"), DEMO_README.as_bytes())?;
        Ok(())
    }
    pub async fn open(config_path: &Path) -> Result<Self> {
        check_path(config_path)?;
        let bytes = read_file(config_path, CONFIG_LIMIT)?;
        let v = canonical::parse(&bytes).map_err(|_| {
            LocalError::Config(
                "ledger.yaml must contain strict JSON (the supported YAML 1.2 subset)",
            )
        })?;
        keys(
            &v,
            &["schema", "mode", "identity", "storage", "auth", "dispatch"],
        )?;
        keys(&v["identity"], &["tenant", "environment", "store_id"])?;
        keys(&v["storage"], &["backend", "data_dir"])?;
        keys(
            &v["auth"],
            &[
                "principal_id",
                "source",
                "authority_head",
                "binding_selector",
            ],
        )?;
        keys(&v["dispatch"], &["enabled", "destination"])?;
        if v["schema"] != "ledger/v1"
            || v["mode"] != "sandbox"
            || v["storage"]["backend"] != "sqlite"
            || v["dispatch"]["enabled"] != false
            || v["dispatch"]["destination"] != "fake"
        {
            return Err(LocalError::Config("unsupported config: ledger/v1 local SQLite sandbox with dispatch disabled is required"));
        }
        let identity = &v["identity"];
        let auth = &v["auth"];
        let store_id = field(identity, "store_id")?;
        let scope = Scope::new(field(identity, "tenant")?, field(identity, "environment")?)
            .map_err(|_| LocalError::Config("invalid config scope"))?;
        let principal = PrincipalContext {
            scope,
            principal_id: field(auth, "principal_id")?.into(),
            source: field(auth, "source")?.into(),
            authority_head: field(auth, "authority_head")?.into(),
            can_submit: true,
            can_read: true,
        };
        let selector = field(auth, "binding_selector")?.into();
        let relative = Path::new(field(&v["storage"], "data_dir")?);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(LocalError::Config(
                "storage.data_dir must be a relative path without traversal",
            ));
        }
        let parent = config_path.parent().unwrap_or(Path::new("."));
        let data = parent.join(relative);
        check_path(&data)?;
        // Reject unsafe or absent files before opening; startup never creates a database.
        private_existing(&data, true)?;
        private_existing(&data.join("local.db"), false)?;
        let store = SqliteStore::open(&data).await.map_err(store_error)?;
        let installation = match store.local_installation().await.map_err(store_error) {
            Ok(v) => v,
            Err(e) => {
                store.close().await;
                return Err(e.into());
            }
        };
        if installation.scope.tenant != principal.scope.tenant()
            || installation.scope.environment != principal.scope.environment()
            || installation.logical_store_id != store_id
            || installation.mode != "sandbox"
            || installation.dispatch_enabled
            || !installation.dispatch_hold
        {
            store.close().await;
            return Err(LocalError::Config(
                "config identity, mode, or dispatch does not match the retained installation",
            ));
        }
        Ok(Self {
            ledger: Ledger {
                store: Backend::Sqlite(store),
            },
            principal,
            selector,
        })
    }
    fn command(&self, bytes: Vec<u8>) -> Result<AcceptCommand> {
        Ok(AcceptCommand {
            bytes,
            principal: self.principal.clone(),
            binding_selector: self.selector.clone(),
            received_at: now()?,
        })
    }
    pub async fn accept(&self, bytes: Vec<u8>) -> Result<AcceptResult> {
        Ok(self.ledger.accept(self.command(bytes)?).await?)
    }
    pub async fn preview(&self, bytes: Vec<u8>) -> Result<PreviewResult> {
        Ok(self.ledger.preview(self.command(bytes)?).await?)
    }
    /// Local filesystem access is the sandbox read authority. This API is not a
    /// remote authenticated read surface and must not be exposed over HTTP.
    pub async fn explain(&self, target: ExplainTarget) -> Result<Value> {
        let Backend::Sqlite(store) = &self.ledger.store else {
            unreachable!("local SQLite only")
        };
        let scope = records::Scope {
            tenant: self.principal.scope.tenant().into(),
            environment: self.principal.scope.environment().into(),
        };
        Ok(inspect::read(store, &scope, &self.principal.source, &target).await?)
    }
    pub async fn close(self) {
        self.ledger.close().await;
    }
}
fn field<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or(LocalError::Config(
            "config requires nonempty bounded strings",
        ))
}
fn keys(v: &Value, expected: &[&str]) -> Result<()> {
    let o = v
        .as_object()
        .ok_or(LocalError::Config("config group must be an object"))?;
    if o.len() != expected.len() || o.keys().any(|k| !expected.contains(&k.as_str())) {
        return Err(LocalError::Config("unknown or missing config field"));
    }
    Ok(())
}
pub fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>> {
    check_path(path)?;
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(LocalError::Config("input must be a regular file"));
    }
    read_bounded(File::open(path)?, limit)
}
pub fn read_bounded(reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(LocalError::Config(
            "input exceeds the documented byte limit",
        ));
    }
    Ok(bytes)
}
fn check_path(path: &Path) -> Result<()> {
    let mut prefix = PathBuf::new();
    for c in path.components() {
        if c == Component::ParentDir {
            return Err(LocalError::Config(
                "parent traversal is not supported in local paths",
            ));
        }
        prefix.push(c);
        match fs::symlink_metadata(&prefix) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(LocalError::Config(
                    "symlinks are not supported in local paths",
                ))
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn private_existing(path: &Path, directory: bool) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    if m.file_type().is_symlink() || (directory && !m.is_dir()) || (!directory && !m.is_file()) {
        return Err(LocalError::Config("invalid local storage path"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if m.permissions().mode() & 0o077 != 0 {
            return Err(LocalError::Config(
                "local storage must be private: directories 0700, database 0600",
            ));
        }
    }
    Ok(())
}
fn private_dir(path: &Path) -> Result<()> {
    let mut options = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        options.mode(0o700);
    }
    options.create(path)?;
    Ok(())
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}
fn now() -> Result<Timestamp> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| LocalError::Config("system clock precedes Unix epoch"))?;
    timestamp(elapsed.as_secs(), elapsed.subsec_micros())
}
fn timestamp(seconds: u64, micros: u32) -> Result<Timestamp> {
    // Gregorian calendar conversion in the environmental facade, never the core.
    let mut days = seconds / 86400;
    let mut year = 1970u64;
    let leap = |y: u64| y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
    while days >= if leap(year) { 366 } else { 365 } {
        days -= if leap(year) { 366 } else { 365 };
        year += 1;
        if year > 9999 {
            return Err(LocalError::Config("system clock exceeds supported year"));
        }
    }
    let months = [
        31,
        if leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0;
    while days >= months[month] {
        days -= months[month];
        month += 1;
    }
    let within = seconds % 86400;
    Timestamp::parse(&format!(
        "{year:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{micros:06}Z",
        month + 1,
        days + 1,
        within / 3600,
        (within / 60) % 60,
        within % 60
    ))
    .map_err(|_| LocalError::Config("invalid system timestamp"))
}
const DEMO_README: &str = "# Ledger Lab local demo\n\nRun `ledger preview examples/generated.json`, then `ledger accept examples/generated.json` and `ledger explain --chain demo-slice`. Retry accept to see the same receipt. Add `--format json` for automation.\n\nSynthetic generation: 100 USD atoms charged, -20 enterprise discount, 80 held for fake export. No payment or export delivery occurs. All fixtures use a fixed demo/sandbox namespace and must never be combined with other ledgers.\n\nThe product story is an AI generation charged initially, with a later linked outcome adding a premium or discount. Optional paid tools and BYOK/platform-funded responsibility fit the same chain. Local commands currently support the generation demo. Linked events, outcomes and reversals cannot yet be saved or previewed. BYOK here creates no host supplier payable.\n\nledger.yaml uses JSON syntax (a YAML 1.2 subset), schema ledger/v1. It contains local host identity, no credentials. Pricing and accepted synthetic terms live in SQLite; editing config cannot reprice history. Keep the whole private .ledger directory together.\n";
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn facade_clock_conversion() {
        for (seconds, s) in [
            (0, "1970-01-01T00:00:00.000000Z"),
            (951782400, "2000-02-29T00:00:00.000000Z"),
            (4107542400, "2100-03-01T00:00:00.000000Z"),
        ] {
            assert_eq!(timestamp(seconds, 0).unwrap(), Timestamp::parse(s).unwrap());
        }
    }
}
