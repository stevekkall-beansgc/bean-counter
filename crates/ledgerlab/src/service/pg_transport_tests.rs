//! Opaque TLS relay: cut real bytes without replacing any production result.
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{watch, Notify},
    task::JoinHandle,
};

pub(super) struct Relay {
    pub port: u16,
    // 0 passes bytes, 1 cuts the next client request, 2 withholds server replies.
    pub mode: Arc<AtomicU8>,
    pub intercepted: Arc<Notify>,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}
impl Relay {
    pub async fn start(upstream: u16) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mode = Arc::new(AtomicU8::new(0));
        let intercepted = Arc::new(Notify::new());
        let (stop, mut stopping) = watch::channel(false);
        let mode_task = mode.clone();
        let intercepted_task = intercepted.clone();
        let task = tokio::spawn(async move {
            let mut connections = Vec::new();
            loop {
                let (client, _) = tokio::select! {
                    accepted = listener.accept() => accepted.unwrap(),
                    _ = stopping.changed() => break,
                };
                let mode = mode_task.clone();
                let intercepted = intercepted_task.clone();
                let mut stopping = stopping.clone();
                connections.push(tokio::spawn(async move {
                    let server = TcpStream::connect(("127.0.0.1", upstream)).await.unwrap();
                    let (mut cr, mut cw) = client.into_split();
                    let (mut sr, mut sw) = server.into_split();
                    let requests = async {
                        let mut buffer = [0; 16384];
                        loop {
                            let n = cr.read(&mut buffer).await?;
                            if n == 0 { return Ok::<_, std::io::Error>(()); }
                            if mode.load(Ordering::SeqCst) == 1 {
                                intercepted.notify_one();
                                return Ok(());
                            }
                            sw.write_all(&buffer[..n]).await?;
                        }
                    };
                    let responses = async {
                        let mut buffer = [0; 16384];
                        loop {
                            let n = sr.read(&mut buffer).await?;
                            if n == 0 { return Ok::<_, std::io::Error>(()); }
                            if mode.load(Ordering::SeqCst) == 2 {
                                intercepted.notify_one();
                                std::future::pending::<()>().await;
                            }
                            cw.write_all(&buffer[..n]).await?;
                        }
                    };
                    tokio::select! { _ = requests => {}, _ = responses => {}, _ = stopping.changed() => {} }
                }));
            }
            for task in connections {
                task.await.unwrap();
            }
        });
        Self {
            port,
            mode,
            intercepted,
            stop,
            task,
        }
    }
    pub async fn cut(self) {
        self.stop.send(true).unwrap();
        self.task.await.unwrap();
    }
}
