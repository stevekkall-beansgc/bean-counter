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
