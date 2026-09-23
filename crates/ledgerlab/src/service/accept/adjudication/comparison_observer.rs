//! Actual SQLx driver events, isolated in a child test process. No production hook.
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use tracing::{
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    Event, Metadata, Subscriber,
};
#[derive(Default)]
pub(super) struct Audit {
    enabled: AtomicBool,
    statements: Mutex<Vec<String>>,
}
struct Capture(Arc<Audit>);
impl Subscriber for Capture {
    fn enabled(&self, m: &Metadata<'_>) -> bool {
        m.target() == "sqlx::query"
    }
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Id::from_u64(NEXT.fetch_add(1, Ordering::Relaxed))
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        if !self.0.enabled.load(Ordering::SeqCst) || event.metadata().target() != "sqlx::query" {
            return;
        }
        #[derive(Default)]
        struct Fields {
            sql: String,
            summary: String,
        }
        impl Visit for Fields {
            fn record_str(&mut self, field: &Field, value: &str) {
                match field.name() {
                    "db.statement" => self.sql = value.into(),
                    "summary" => self.summary = value.into(),
                    _ => {}
                }
            }
            fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
                match field.name() {
                    "db.statement" => self.sql = format!("{value:?}"),
                    "summary" => self.summary = format!("{value:?}"),
                    _ => {}
                }
            }
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let sql = if fields.sql.trim().is_empty() {
            fields.summary
        } else {
            fields.sql
        };
        if !sql.is_empty() {
            self.0.statements.lock().unwrap().push(sql);
        }
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}
impl Audit {
    pub(super) fn install() -> Arc<Self> {
        let audit = Arc::new(Self::default());
        tracing::subscriber::set_global_default(Capture(audit.clone())).unwrap();
        audit
    }
    pub(super) fn begin(&self) {
        self.statements.lock().unwrap().clear();
        self.enabled.store(true, Ordering::SeqCst);
    }
    pub(super) fn end(&self) -> Vec<String> {
        self.enabled.store(false, Ordering::SeqCst);
        std::mem::take(&mut *self.statements.lock().unwrap())
    }
}
pub(super) fn business_write(sql: &str) -> bool {
    sql.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|word| {
            matches!(
                word.to_ascii_uppercase().as_str(),
                "UPDATE"
                    | "INSERT"
                    | "DELETE"
                    | "REPLACE"
                    | "CREATE"
                    | "ALTER"
                    | "DROP"
                    | "ATTACH"
                    | "DETACH"
                    | "VACUUM"
                    | "REINDEX"
                    | "ANALYZE"
            )
        })
}
#[test]
fn actual_comparison_driver_observation() {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "service::accept::adjudication::sqlite_tests::actual_customer_ninety_five_commands",
            "--nocapture",
        ])
        .env("LEDGERLAB_R3_SQL_OBSERVE", "1")
        .spawn()
        .unwrap();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(180);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if std::time::Instant::now() >= end {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("comparison observer child deadline")
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}
