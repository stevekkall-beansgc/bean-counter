//! The worker owns the snapshot, so an idle caller cannot extend its lifetime.
//! It retains the profile read gate until rollback/discard cleanup has run.
use super::*;
use tokio::sync::{mpsc, oneshot, OwnedRwLockReadGuard};
enum Request {
    Authority(
        AuthenticatedReadContext,
        oneshot::Sender<Result<ReadAuthorityObservation>>,
    ),
    Retained(
        RetainedSelection,
        oneshot::Sender<Result<RawRetainedSnapshot>>,
    ),
    Finish(oneshot::Sender<Result<()>>),
}
pub(super) struct Bounded {
    send: mpsc::Sender<Request>,
    deadline: Instant,
}
impl Bounded {
    pub(super) async fn authority(
        &self,
        who: AuthenticatedReadContext,
    ) -> Result<ReadAuthorityObservation> {
        let (reply, receive) = oneshot::channel();
        self.send
            .try_send(Request::Authority(who, reply))
            .map_err(|_| ReadError::Unavailable)?;
        timeout_at(self.deadline, receive)
            .await
            .map_err(|_| ReadError::Deadline)?
            .map_err(|_| ReadError::Unavailable)?
    }
    pub(super) async fn retained(
        &self,
        selection: RetainedSelection,
    ) -> Result<RawRetainedSnapshot> {
        let (reply, receive) = oneshot::channel();
        self.send
            .try_send(Request::Retained(selection, reply))
            .map_err(|_| ReadError::Unavailable)?;
        timeout_at(self.deadline, receive)
            .await
            .map_err(|_| ReadError::Deadline)?
            .map_err(|_| ReadError::Unavailable)?
    }
    pub(super) async fn finish(self) -> Result<()> {
        let (reply, receive) = oneshot::channel();
        self.send
            .try_send(Request::Finish(reply))
            .map_err(|_| ReadError::Unavailable)?;
        timeout_at(self.deadline + Duration::from_secs(2), receive)
            .await
            .map_err(|_| ReadError::Deadline)?
            .map_err(|_| ReadError::Unavailable)?
    }
}
async fn cleanup(read: &mut SqliteComparisonRead) -> Result<()> {
    let tx = read.transaction.take().ok_or(ReadError::Unavailable)?;
    timeout_at(Instant::now() + Duration::from_secs(2), tx.rollback())
        .await
        .map_err(|_| ReadError::Deadline)?
        .map_err(db)
}
pub(super) fn spawn(
    mut session: SqliteComparisonRead,
    lane: OwnedRwLockReadGuard<()>,
) -> SqliteComparisonRead {
    let (send, mut receive) = mpsc::channel(1);
    let deadline = session.deadline;
    let read = SqliteComparisonRead {
        transaction: None,
        deadline,
        budget: ReadBudget::default(),
        statements: session.statements.clone(),
        ready: true,
        authority_scope: None,
        bounded: Some(Bounded { send, deadline }),
    };
    tokio::spawn(async move {
        let _lane = lane;
        loop {
            let request = tokio::select! {biased;_ = tokio::time::sleep_until(deadline)=>None,r=receive.recv()=>r};
            let Some(request) = request else {
                break;
            };
            match request {
                Request::Authority(who, mut reply) => {
                    let result = tokio::select! {biased;_=reply.closed()=>Err(ReadError::Cancelled),r=session.load_authority(&who)=>r};
                    let stop = result.is_err();
                    if reply.send(result).is_err() || stop {
                        break;
                    }
                }
                Request::Retained(selection, mut reply) => {
                    let result = tokio::select! {biased;_=reply.closed()=>Err(ReadError::Cancelled),r=session.load_retained(&selection)=>r};
                    let stop = result.is_err();
                    if reply.send(result).is_err() || stop {
                        break;
                    }
                }
                Request::Finish(reply) => {
                    let result = cleanup(&mut session).await;
                    let _ = reply.send(result);
                    return;
                }
            }
        }
        let _ = cleanup(&mut session).await;
    });
    read
}
