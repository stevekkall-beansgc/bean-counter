//! Explicit TCP configuration and a dedicated connection with discard-on-drop.
use std::time::Duration;

use rustls::pki_types::ServerName;
use tokio::{task::JoinHandle, time::timeout};
use tokio_postgres::{config::SslMode, Client, Config, Error};
use tokio_postgres_rustls::MakeRustlsConnect;

pub const CONNECT_BOUND: Duration = Duration::from_secs(3);
pub const STARTUP_OPTIONS: &str = "-c search_path=pg_catalog -c synchronous_commit=on -c statement_timeout=2000 -c lock_timeout=500 -c idle_in_transaction_session_timeout=5000";

// No Debug: credentials must not appear in error/log output.
pub struct Settings<'a> {
    pub host: &'a str,
    pub port: u16,
    pub user: &'a str,
    pub password: &'a [u8],
    pub database: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidSettings;

pub(crate) fn config(settings: &Settings<'_>) -> Result<Config, InvalidSettings> {
    // A validated DNS name/IP also excludes Unix paths, comma-separated hosts,
    // URL parameters and empty host defaults. No URL parser in this proof.
    ServerName::try_from(settings.host).map_err(|_| InvalidSettings)?;
    if settings.port == 0
        || settings.user.is_empty()
        || settings.database.is_empty()
        || settings.user.contains('\0')
        || settings.database.contains('\0')
    {
        return Err(InvalidSettings);
    }
    let mut config = Config::new();
    config
        .host(settings.host)
        .port(settings.port)
        .user(settings.user)
        .password(settings.password)
        .dbname(settings.database)
        .ssl_mode(SslMode::Require)
        .application_name("ledgerlab-driver-proof")
        .options(STARTUP_OPTIONS)
        .connect_timeout(CONNECT_BOUND);
    Ok(config)
}

#[derive(Debug)]
pub enum ConnectError {
    InvalidSettings,
    Deadline,
    Driver(Error),
}

pub struct Session {
    pub(crate) client: Client,
    driver: JoinHandle<Result<(), Error>>,
}

impl Session {
    pub async fn open(
        settings: &Settings<'_>,
        connector: MakeRustlsConnect,
    ) -> Result<Self, ConnectError> {
        let config = config(settings).map_err(|_| ConnectError::InvalidSettings)?;
        let (client, connection) = timeout(CONNECT_BOUND, config.connect(connector))
            .await
            .map_err(|_| ConnectError::Deadline)?
            .map_err(ConnectError::Driver)?;
        Ok(Self {
            client,
            driver: tokio::spawn(connection),
        })
    }

    /// Close and join the driver before reporting discard complete. This does
    /// not assert server rollback or resolve a commit's outcome.
    pub async fn discard(mut self) {
        self.driver.abort();
        let _ = (&mut self.driver).await;
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // A dropped JoinHandle detaches by default. Explicit abort instead
        // ensures cancellation cannot leave an unbounded live connection task.
        // Runtime scheduling completes the abort; discard() additionally joins.
        self.driver.abort();
    }
}
