//! Instrumentation is compiled out of production. Tests pause actual polled futures.
use crate::ServiceError;
use std::future::Future;
#[derive(Clone, Default)]
pub(crate) struct Hooks {
    #[cfg(test)]
    pub state: std::sync::Arc<TestState>,
}
impl Hooks {
    pub async fn call<T: Send>(&self, name: &str, future: impl Future<Output = T> + Send) -> T {
        #[cfg(not(test))]
        let _ = name;
        #[cfg(test)]
        if let Some(point) = &self.state.cancel {
            if point.name == name {
                // Poll the real operation before stopping the caller. The driver may
                // finish a statement; its transaction must still rollback as a whole.
                let mut future = std::pin::pin!(future);
                let polled =
                    std::future::poll_fn(|cx| std::task::Poll::Ready(future.as_mut().poll(cx)))
                        .await;
                self.mark(ledgerlab_testkit::failpoints::Injection {
                    boundary: ledgerlab_testkit::failpoints::Boundary::Await {
                        name: name.into(),
                        phase: point.phase,
                    },
                    fault: ledgerlab_testkit::failpoints::Fault::Cancel,
                });
                self.state.reached.notify_one();
                std::future::pending::<()>().await;
                return match polled {
                    std::task::Poll::Ready(v) => v,
                    std::task::Poll::Pending => future.await,
                };
            }
        }
        future.await
    }
    pub fn evaluate(
        &self,
        input: &ledgerlab_core::domain::ResolvedInput,
    ) -> ledgerlab_core::Result<ledgerlab_core::domain::DecisionPlan> {
        #[cfg(test)]
        if let Some(i) = &self.state.injection {
            if i.boundary == Boundary::EvaluatorAfterBase {
                let fault = match i.fault {
                    Fault::EvaluationInvalid => {
                        ledgerlab_core::policy::EvaluationFault::InvalidAfterBase
                    }
                    Fault::EvaluationOverflow => {
                        ledgerlab_core::policy::EvaluationFault::OverflowAfterBase
                    }
                    _ => panic!("invalid evaluator injection"),
                };
                let result = ledgerlab_core::policy::evaluate_with_fault(input, fault);
                if result
                    .as_ref()
                    .is_err_and(|e| matches!(e.code, "EVALUATION_INVALID" | "ARITHMETIC_OVERFLOW"))
                {
                    self.mark(i.clone());
                }
                return result;
            }
        }
        ledgerlab_core::policy::evaluate(input)
    }
    pub fn write(&self, name: &str, item: usize, after: bool) -> Result<(), ServiceError> {
        #[cfg(not(test))]
        let _ = (name, item, after);
        #[cfg(test)]
        self.check(ledgerlab_testkit::failpoints::Boundary::Write {
            name: name.into(),
            item,
            edge: if after {
                ledgerlab_testkit::failpoints::Edge::After
            } else {
                ledgerlab_testkit::failpoints::Edge::Before
            },
        })?;
        Ok(())
    }
    pub fn before_commit(&self) -> Result<(), ServiceError> {
        #[cfg(test)]
        self.check(ledgerlab_testkit::failpoints::Boundary::BeforeCommitSend)?;
        Ok(())
    }
    pub fn after_commit(&self) -> Result<(), ServiceError> {
        #[cfg(test)]
        self.check(ledgerlab_testkit::failpoints::Boundary::AfterCommitAcknowledged)?;
        Ok(())
    }
}
#[cfg(test)]
use ledgerlab_testkit::failpoints::{Boundary, CancellationPoint, Fault, Hit, Injection};
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestState {
    pub injection: Option<Injection>,
    pub cancel: Option<CancellationPoint>,
    pub hit: std::sync::Mutex<Option<Hit>>,
    pub reached: tokio::sync::Notify,
    pub release: tokio::sync::Notify,
    pub active: std::sync::atomic::AtomicBool,
    pub task: std::sync::Mutex<
        Option<tokio::task::JoinHandle<Result<(), crate::store::errors::CommitError>>>,
    >,
}
#[cfg(test)]
impl Hooks {
    pub fn new(injection: Option<Injection>, cancel: Option<CancellationPoint>) -> Self {
        Self {
            state: std::sync::Arc::new(TestState {
                injection,
                cancel,
                ..Default::default()
            }),
        }
    }
    pub fn mark(&self, injection: Injection) {
        if self.state.cancel.is_some() && injection.fault != Fault::Cancel {
            return;
        }
        let mut h = self.state.hit.lock().unwrap();
        match &mut *h {
            Some(h) => h.hits += 1,
            None => *h = Some(Hit { injection, hits: 1 }),
        }
    }
    pub fn hit(&self) -> Option<Hit> {
        self.state.hit.lock().unwrap().clone()
    }
    fn check(&self, boundary: Boundary) -> Result<(), ServiceError> {
        if let Some(injection) = &self.state.injection {
            if injection.boundary == boundary {
                self.mark(injection.clone());
                return Err(match injection.fault {
                    Fault::LoseReply => ServiceError::ResponseLost,
                    _ => ServiceError::Injected,
                });
            }
        }
        Ok(())
    }
    pub fn unknown(&self) -> Option<bool> {
        let i = self.state.injection.as_ref()?;
        if i.boundary != Boundary::CommitInFlight {
            return None;
        }
        match i.fault {
            Fault::UnknownCommit { durable } => Some(durable),
            Fault::UnknownCommitActive => Some(true),
            _ => None,
        }
    }
    pub fn hold_unknown<T: crate::store::ports::AcceptanceTx + 'static>(
        &self,
        tx: T,
        durable: bool,
    ) {
        self.mark(self.state.injection.clone().unwrap());
        let state = self.state.clone();
        state
            .active
            .store(true, std::sync::atomic::Ordering::Release);
        let task = tokio::spawn(async move {
            // Cancellation of the caller cannot orphan the owned transaction.
            // A supervisor always releases it or the five-second deadline drains it.
            let release =
                tokio::time::timeout(std::time::Duration::from_secs(5), state.release.notified())
                    .await;
            let result = if release.is_ok() && durable {
                tx.commit().await
            } else {
                tx.rollback()
                    .await
                    .map_err(crate::store::errors::CommitError::RolledBack)
            };
            state
                .active
                .store(false, std::sync::atomic::Ordering::Release);
            result
        });
        *self.state.task.lock().unwrap() = Some(task);
    }
    pub async fn drain(&self) {
        self.state.release.notify_one();
        let task = self.state.task.lock().unwrap().take();
        if let Some(task) = task {
            let _ = task.await.expect("bounded fault supervisor");
        }
    }
}
