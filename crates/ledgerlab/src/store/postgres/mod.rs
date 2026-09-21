//! Concrete PostgreSQL adapter. A bounded lease owns a dedicated verified session;
//! it is discarded after each transaction, so uncertain sessions cannot be reused.
#[path = "proof/connect.rs"]
pub(crate) mod connect;
pub(crate) mod migrate;
mod outbox;
mod outcomes;
mod read;
#[path = "proof/tls.rs"]
pub(crate) mod tls;
mod tx;
pub(crate) mod write;

#[cfg(test)]
use crate::store::records::*;
use crate::store::{errors::StoreError, ports::AcceptanceStore};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    sync::Semaphore,
    task::JoinHandle,
    time::{timeout_at, Instant},
};
pub(crate) use tx::PostgresTx;

#[derive(Clone)]
pub enum PostgresTrust {
    Public,
    PemOnly(Vec<u8>),
}
/// Fully resolved TCP settings. No URL parser, environment lookup, or Debug output.
#[derive(Clone)]
pub struct PostgresConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Vec<u8>,
    pub database: String,
    pub trust: PostgresTrust,
}
impl PostgresConfig {
    pub(crate) async fn connect(&self) -> Result<connect::Session, StoreError> {
        let connector = tls::connector(match &self.trust {
            PostgresTrust::Public => tls::Trust::Public,
            PostgresTrust::PemOnly(pem) => tls::Trust::PemOnly(pem),
        })
        .map_err(|_| StoreError::InvalidStore("invalid explicit TLS trust"))?;
        connect::Session::open(
            &connect::Settings {
                host: &self.host,
                port: self.port,
                user: &self.user,
                password: &self.password,
                database: &self.database,
            },
            connector,
        )
        .await
        .map_err(|e| match e {
            connect::ConnectError::Driver(e) => StoreError::Postgres(e),
            connect::ConnectError::Deadline => StoreError::Deadline,
            connect::ConnectError::InvalidSettings => {
                StoreError::InvalidStore("explicit PostgreSQL settings required")
            }
        })
    }
}
struct Inner {
    config: PostgresConfig,
    slots: Arc<Semaphore>,
    closed: AtomicBool,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    draining: tokio::sync::Mutex<Vec<JoinHandle<()>>>,
    #[cfg(test)]
    last_pid: std::sync::atomic::AtomicI32,
}
impl Drop for Inner {
    fn drop(&mut self) {
        for task in self.tasks.get_mut().expect("task registry").drain(..) {
            task.abort();
        }
        for task in self.draining.get_mut().drain(..) {
            task.abort();
        }
    }
}
#[derive(Clone)]
pub(crate) struct PostgresStore {
    inner: Arc<Inner>,
    #[cfg(test)]
    pub version: i32,
}
impl PostgresStore {
    pub async fn open(config: PostgresConfig) -> Result<Self, StoreError> {
        let session = config.connect().await?;
        let version = verify_server(&session.client).await?;
        #[cfg(not(test))]
        let _ = version;
        migrate::verify(&session.client).await?;
        verify_role(&session.client).await?;
        #[cfg(test)]
        let pid: i32 = session
            .client
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .try_get(0)?;
        session.discard().await;
        Ok(Self {
            inner: Arc::new(Inner {
                config,
                slots: Arc::new(Semaphore::new(5)),
                closed: AtomicBool::new(false),
                tasks: Mutex::new(Vec::new()),
                draining: tokio::sync::Mutex::new(Vec::new()),
                #[cfg(test)]
                last_pid: std::sync::atomic::AtomicI32::new(pid),
            }),
            #[cfg(test)]
            version,
        })
    }
    #[cfg(test)]
    pub(crate) fn test_pid(&self) -> i32 {
        self.inner.last_pid.load(Ordering::Acquire)
    }
    pub async fn close(self) {
        self.inner.closed.store(true, Ordering::Release);
        self.inner.slots.close();
        // All close callers await the same drain. Keep handles in the owner
        // across awaits so cancelling one close cannot detach supervised work.
        let mut draining = self.inner.draining.lock().await;
        draining.extend(std::mem::take(
            &mut *self.inner.tasks.lock().expect("task registry"),
        ));
        while let Some(task) = draining.last_mut() {
            let _ = task.await;
            draining.pop();
        }
    }
    /// Primary lookup is observational only: absence is never a rollback result.
    #[cfg(test)]
    pub async fn lookup_identity(
        &self,
        s: &Scope,
        source: &str,
        external: &str,
    ) -> Result<Option<StoredIdentity>, StoreError> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let _permit = timeout_at(deadline, self.inner.slots.clone().acquire_owned())
            .await
            .map_err(|_| StoreError::Deadline)?
            .map_err(|_| StoreError::WritesDisabled)?;
        timeout_at(deadline, async {
            let session = self.inner.config.connect().await?;
            verify_server(&session.client).await?;
            let result = read::identity(&session.client, s, source, external).await;
            session.discard().await;
            result
        })
        .await
        .map_err(|_| StoreError::Deadline)?
    }
}
impl AcceptanceStore for PostgresStore {
    type Tx = PostgresTx;
    async fn begin(&self, deadline: Instant) -> Result<PostgresTx, StoreError> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(StoreError::WritesDisabled);
        }
        let permit = timeout_at(
            deadline.min(Instant::now() + Duration::from_secs(1)),
            self.inner.slots.clone().acquire_owned(),
        )
        .await
        .map_err(|_| StoreError::Deadline)?
        .map_err(|_| StoreError::WritesDisabled)?;
        tx::start(self.inner.clone(), permit, deadline).await
    }
}
pub(crate) async fn verify_server<C: tokio_postgres::GenericClient + Sync>(
    client: &C,
) -> Result<i32, StoreError> {
    let row=client.query_one("SELECT current_setting('server_version_num')::int4, pg_is_in_recovery(), current_setting('transaction_read_only'), current_setting('fsync'), current_setting('full_page_writes'), current_setting('synchronous_commit'), (SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid=pg_backend_pid())",&[]).await?;
    let version: i32 = row.try_get(0)?;
    if !(170000..190000).contains(&version)
        || row.try_get::<_, bool>(1)?
        || row.try_get::<_, String>(2)? != "off"
        || (3..6).any(|i| row.get::<_, String>(i) != "on")
        || !row.try_get::<_, bool>(6)?
    {
        return Err(StoreError::InvalidStore(
            "primary, verified TLS and durable PostgreSQL 17/18 required",
        ));
    }
    Ok(version)
}
async fn verify_role(client: &tokio_postgres::Client) -> Result<(), StoreError> {
    let unsafe_role:bool=client.query_one("SELECT r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR has_schema_privilege(current_user,'ledgerlab','CREATE') OR has_schema_privilege(current_user,'public','CREATE') OR EXISTS(SELECT 1 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='ledgerlab' AND (pg_has_role(current_user,c.relowner,'USAGE') OR (c.relkind='r' AND (has_table_privilege(current_user,c.oid,'TRUNCATE') OR has_table_privilege(current_user,c.oid,'DELETE'))))) FROM pg_catalog.pg_roles r WHERE r.rolname=current_user",&[]).await?.try_get(0)?;
    if unsafe_role {
        return Err(StoreError::InvalidStore(
            "PostgreSQL runtime must not own schema/tables or have DDL/destructive privileges",
        ));
    }
    Ok(())
}

impl crate::store::outcomes::OutcomeStore for PostgresStore {
    type Tx = PostgresTx;
    async fn begin_outcome(&self, deadline: Instant) -> Result<PostgresTx, StoreError> {
        self.begin(deadline).await
    }
}
