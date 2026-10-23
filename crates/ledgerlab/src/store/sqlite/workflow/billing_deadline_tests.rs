use super::until;
use std::{cell::Cell, future::pending, time::Duration};
use tokio::time::Instant;

#[tokio::test]
async fn expired_deadline_rejects_ready_work_without_polling_it() {
    let polled = Cell::new(false);
    let result = until(Instant::now() - Duration::from_secs(1), async {
        polled.set(true);
        42
    })
    .await;

    assert_eq!((result.is_err(), polled.get()), (true, false));
}

#[tokio::test]
async fn unlimited_work_preserves_its_result() {
    let result = until(None, async { Err::<(), _>("storage refusal") }).await;
    assert_eq!(result.unwrap(), Err("storage refusal"));
}

#[tokio::test]
async fn live_deadline_allows_ready_work() {
    let result = until(Instant::now() + Duration::from_secs(300), async { 42 }).await;
    assert_eq!(result.unwrap(), 42);
}

#[tokio::test]
async fn live_deadline_still_times_out_pending_work() {
    let result = until(Instant::now() + Duration::from_millis(1), pending::<()>()).await;
    assert!(result.is_err());
}
