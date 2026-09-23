//! Test-only bounded Phase 3 diagnosis. No credentials or request bodies.
use std::{
    fmt,
    future::Future,
    time::{SystemTime, UNIX_EPOCH},
};
tokio::task_local! { static LABEL: String; }
pub(crate) fn enabled() -> bool {
    std::env::var_os("LEDGERLAB_R2_TRACE").is_some()
}
pub(crate) fn label() -> String {
    LABEL
        .try_with(Clone::clone)
        .unwrap_or_else(|_| "unlabelled".into())
}
pub(crate) async fn scope<T>(label: String, future: impl Future<Output = T>) -> T {
    LABEL.scope(label, future).await
}
pub(crate) fn log(message: impl fmt::Display) {
    if enabled() {
        eprintln!(
            "R2 us={} label={} {}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros(),
            label(),
            message
        );
    }
}

pub(crate) struct Span(&'static str, std::time::Instant);
impl Span {
    pub(crate) fn new(name: &'static str) -> Self {
        Self(name, std::time::Instant::now())
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if label() != "unlabelled" {
            log(format_args!(
                "sync_phase={} elapsed_us={}",
                self.0,
                self.1.elapsed().as_micros()
            ));
        }
    }
}

/// Actual process-kill rendezvous, compiled only in the test module. The parent
/// selects one exact child phase and kills it; this never simulates SQL success.
pub(crate) async fn publication_cut(phase: u8) {
    if std::env::var("LEDGERLAB_PG_PUBLICATION_CUT")
        .ok()
        .and_then(|v| v.parse::<u8>().ok())
        != Some(phase)
    {
        return;
    }
    use std::io::Write;
    let path = std::env::var_os("LEDGERLAB_PG_PUBLICATION_READY").expect("child ready path");
    let mut file = std::fs::File::create(path).expect("child ready file");
    file.write_all(format!("{}:{phase}", std::process::id()).as_bytes())
        .unwrap();
    file.sync_all().unwrap();
    std::future::pending::<()>().await;
}
