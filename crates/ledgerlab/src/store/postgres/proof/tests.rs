//! Synthetic peers exercise TLS and wire/lifetime behavior only, never durability.
use crate::{
    connect::{config, Session, Settings, STARTUP_OPTIONS},
    tls::{connector, root_store, Trust},
    tx::{self, CommitOutcome, ProbeOutcome},
};
use rcgen::{
    date_time_ymd, BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose,
};
use rustls::{
    pki_types::{pem::PemObject, CertificateDer, PrivatePkcs8KeyDer},
    RootCertStore, ServerConfig,
};
use std::{
    error::Error as _,
    process::Command,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
use tokio_postgres::{
    config::{Host, SslMode},
    Error, IsolationLevel,
};
use tokio_rustls::{server::TlsStream, TlsAcceptor};

const SSL_REQUEST: [u8; 8] = [0, 0, 0, 8, 4, 210, 22, 47];
const BOUND: Duration = Duration::from_secs(3);
static NEXT_CA: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root_pem: Vec<u8>,
    server: Arc<ServerConfig>,
}

fn fixture(name: &str, expired: bool) -> Fixture {
    let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.distinguished_name.push(
        DnType::CommonName,
        format!("probe-ca-{}", NEXT_CA.fetch_add(1, Ordering::Relaxed)),
    );
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca_key = KeyPair::generate().unwrap();
    let ca_cert = ca.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca, ca_key);
    let mut leaf = CertificateParams::new(vec![name.to_owned()]).unwrap();
    leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    leaf.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    if expired {
        leaf.not_before = date_time_ymd(2000, 1, 1);
        leaf.not_after = date_time_ymd(2001, 1, 1);
    }
    let leaf_key = KeyPair::generate().unwrap();
    let leaf_cert = leaf.signed_by(&leaf_key, &issuer).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let server = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf_cert.der().clone()],
            PrivatePkcs8KeyDer::from(leaf_key.serialize_der()).into(),
        )
        .unwrap();
    Fixture {
        root_pem: ca_cert.pem().into_bytes(),
        server: Arc::new(server),
    }
}

fn settings(port: u16) -> Settings<'static> {
    Settings {
        host: "127.0.0.1",
        port,
        user: "synthetic-probe",
        password: b"synthetic-password",
        database: "synthetic-db",
    }
}

async fn handshake(
    listener: &TcpListener,
    server: Arc<ServerConfig>,
) -> Option<TlsStream<TcpStream>> {
    let (mut stream, _) = listener.accept().await.unwrap();
    let mut request = [0; 8];
    stream.read_exact(&mut request).await.unwrap();
    assert_eq!(request, SSL_REQUEST);
    stream.write_all(b"S").await.unwrap();
    TlsAcceptor::from(server).accept(stream).await.ok()
}

async fn message(stream: &mut TlsStream<TcpStream>, tag: u8, body: &[u8]) {
    stream.write_u8(tag).await.unwrap();
    stream.write_u32((body.len() + 4) as u32).await.unwrap();
    stream.write_all(body).await.unwrap();
    stream.flush().await.unwrap();
}

async fn startup(stream: &mut TlsStream<TcpStream>) -> bool {
    let Ok(len) = stream.read_u32().await else {
        return false;
    };
    assert!((8..8192).contains(&len));
    let mut body = vec![0; len as usize - 4];
    stream.read_exact(&mut body).await.unwrap();
    assert_eq!(&body[..4], &[0, 3, 0, 0]);
    let fields: Vec<_> = body[4..].split(|b| *b == 0).collect();
    let pairs: Vec<_> = fields
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| (p[0], p[1]))
        .collect();
    assert!(pairs.contains(&(b"user".as_slice(), b"synthetic-probe".as_slice())));
    assert!(pairs.contains(&(b"database".as_slice(), b"synthetic-db".as_slice())));
    assert!(pairs.contains(&(b"options".as_slice(), STARTUP_OPTIONS.as_bytes())));
    true
}

async fn tls_probe(f: Fixture, pem: Option<Vec<u8>>) -> (Error, bool) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let opts = config(&settings(listener.local_addr().unwrap().port())).unwrap();
    let tls = connector(pem.as_deref().map_or(Trust::Public, Trust::PemOnly)).unwrap();
    let server = async {
        let Some(mut stream) = handshake(&listener, f.server).await else {
            return false;
        };
        if !startup(&mut stream).await {
            return false;
        }
        message(
            &mut stream,
            b'E',
            b"SFATAL\0C08004\0MTLS_PROBE_REACHED_STARTUP\0\0",
        )
        .await;
        true
    };
    let (client, reached) = timeout(BOUND, async { tokio::join!(opts.connect(tls), server) })
        .await
        .unwrap();
    let err = match client {
        Err(err) => err,
        Ok(_) => panic!("peer must reject startup"),
    };
    (err, reached)
}

fn error_chain(error: &Error) -> String {
    let mut text = error.to_string();
    let mut next = error.source();
    while let Some(cause) = next {
        text.push_str(&format!(" {cause}"));
        next = cause.source();
    }
    text
}

#[test]
fn pem_only_contains_exact_supplied_anchors_and_no_bundled_public_roots() {
    let f = fixture("127.0.0.1", false);
    let actual = root_store(Trust::PemOnly(&f.root_pem)).unwrap();
    let mut expected = RootCertStore::empty();
    expected
        .add(CertificateDer::from_pem_slice(&f.root_pem).unwrap())
        .unwrap();
    assert_eq!(actual.roots, expected.roots);
    assert_eq!(actual.len(), 1);
    let public = root_store(Trust::Public).unwrap();
    assert_eq!(public.roots, webpki_roots::TLS_SERVER_ROOTS);
    assert!(!public.is_empty());
    for root in &public.roots {
        assert!(!actual.roots.contains(root));
    }
    let other = fixture("127.0.0.1", false);
    let combined = [f.root_pem, other.root_pem].concat();
    assert_eq!(root_store(Trust::PemOnly(&combined)).unwrap().len(), 2);
}

#[test]
fn empty_malformed_or_oversized_pem_fails_closed() {
    for pem in [
        b"".as_slice(),
        b"not a certificate",
        b"-----BEGIN CERTIFICATE-----\n!bad\n-----END CERTIFICATE-----",
    ] {
        assert!(connector(Trust::PemOnly(pem)).is_err());
    }
    assert!(connector(Trust::PemOnly(&vec![b'x'; 65_537])).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn supplied_private_ca_verifies_and_reaches_startup() {
    let f = fixture("127.0.0.1", false);
    let pem = f.root_pem.clone();
    let (error, reached) = tls_probe(f, Some(pem)).await;
    assert!(reached);
    assert_eq!(
        error.as_db_error().unwrap().message(),
        "TLS_PROBE_REACHED_STARTUP"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn wrong_host_expired_leaf_unrelated_ca_and_public_mode_reject() {
    for (host, expired, trust, reason) in [
        ("wrong.invalid", false, 0, "not valid for name"),
        ("127.0.0.1", true, 0, "expired"),
        ("127.0.0.1", false, 1, "UnknownIssuer"),
        ("127.0.0.1", false, 2, "UnknownIssuer"),
    ] {
        let f = fixture(host, expired);
        let pem = match trust {
            0 => Some(f.root_pem.clone()),
            1 => Some(fixture(host, false).root_pem),
            _ => None,
        };
        let (error, reached) = tls_probe(f, pem).await;
        assert!(!reached);
        assert!(
            error_chain(&error).contains(reason),
            "{}",
            error_chain(&error)
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn require_rejects_plaintext_without_startup() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let opts = config(&settings(listener.local_addr().unwrap().port())).unwrap();
    let server = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 8];
        stream.read_exact(&mut request).await.unwrap();
        assert_eq!(request, SSL_REQUEST);
        stream.write_all(b"N").await.unwrap();
        assert_eq!(stream.read(&mut [0; 1]).await.unwrap(), 0);
    };
    let (result, ()) = timeout(BOUND, async {
        tokio::join!(opts.connect(connector(Trust::Public).unwrap()), server)
    })
    .await
    .unwrap();
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("plaintext connection accepted"),
    };
    assert!(error_chain(&err).contains("server does not support TLS"));
}

#[test]
fn rejects_implicit_or_non_tcp_settings() {
    for host in ["", "/tmp", "host1,host2", "host?sslmode=disable"] {
        let mut s = settings(5432);
        s.host = host;
        assert!(config(&s).is_err());
    }
    let mut s = settings(5432);
    s.user = "";
    assert!(config(&s).is_err());
    let mut s = settings(5432);
    s.database = "";
    assert!(config(&s).is_err());
}

#[test]
fn ambient_pg_home_and_ssl_environment_cannot_change_config_or_trust() {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tests::ambient_child", "--ignored"])
        .env_clear()
        .env("PGHOST", "ambient.invalid")
        .env("PGHOSTADDR", "192.0.2.1")
        .env("PGPORT", "5439")
        .env("PGUSER", "ambient-user")
        .env("PGPASSWORD", "ambient-password")
        .env("PGDATABASE", "ambient-db")
        .env("PGSSLMODE", "disable")
        .env("PGOPTIONS", "-c synchronous_commit=off")
        .env("PGAPPNAME", "ambient-app")
        .env("PGSERVICE", "ambient-service")
        .env("PGSERVICEFILE", "/nonexistent/pg_service.conf")
        .env("PGPASSFILE", "/nonexistent/pgpass")
        .env("PGSSLROOTCERT", "/nonexistent/ca")
        .env("PGSSLCERT", "/nonexistent/cert")
        .env("PGSSLKEY", "/nonexistent/key")
        .env("HOME", "/nonexistent/home")
        .env("SSL_CERT_FILE", "/nonexistent/ssl-ca")
        .env("SSL_CERT_DIR", "/nonexistent/ssl")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "parent launches with synthetic environment in a separate process"]
async fn ambient_child() {
    let c = config(&settings(5432)).unwrap();
    assert_eq!(c.get_hosts(), &[Host::Tcp("127.0.0.1".into())]);
    assert!(c.get_hostaddrs().is_empty());
    assert_eq!(c.get_ports(), &[5432]);
    assert_eq!(c.get_user(), Some("synthetic-probe"));
    assert_eq!(c.get_password(), Some(b"synthetic-password".as_slice()));
    assert_eq!(c.get_dbname(), Some("synthetic-db"));
    assert_eq!(c.get_ssl_mode(), SslMode::Require);
    assert_eq!(c.get_options(), Some(STARTUP_OPTIONS));
    assert_eq!(c.get_application_name(), Some("ledgerlab-driver-proof"));
    pem_only_contains_exact_supplied_anchors_and_no_bundled_public_roots();
    supplied_ca_child_probe().await;
}

async fn supplied_ca_child_probe() {
    let f = fixture("127.0.0.1", false);
    let pem = f.root_pem.clone();
    assert!(tls_probe(f, Some(pem)).await.1);
}

#[derive(Clone, Copy)]
enum Behavior {
    Normal,
    LostCommitReply,
    CommitHang,
    CommitError(&'static str),
    BeginHang,
    QueryHang,
}
type Trace = Arc<Mutex<Vec<String>>>;

async fn authenticate(stream: &mut TlsStream<TcpStream>) {
    assert!(startup(stream).await);
    message(stream, b'R', &0u32.to_be_bytes()).await;
    message(
        stream,
        b'K',
        &[123i32.to_be_bytes(), 456i32.to_be_bytes()].concat(),
    )
    .await;
    message(stream, b'Z', b"I").await;
}

async fn receive(stream: &mut TlsStream<TcpStream>) -> Option<(u8, Vec<u8>)> {
    let tag = stream.read_u8().await.ok()?;
    let len = stream.read_u32().await.unwrap();
    assert!((4..8192).contains(&len));
    let mut body = vec![0; len as usize - 4];
    stream.read_exact(&mut body).await.unwrap();
    Some((tag, body))
}

fn cstring<'a>(body: &mut &'a [u8]) -> &'a [u8] {
    let end = body.iter().position(|b| *b == 0).unwrap();
    let value = &body[..end];
    *body = &body[end + 1..];
    value
}
fn take<const N: usize>(body: &mut &[u8]) -> [u8; N] {
    let value = body[..N].try_into().unwrap();
    *body = &body[N..];
    value
}

async fn wait_for_disconnect(stream: &mut TlsStream<TcpStream>, trace: &Trace) {
    while let Some((tag, _)) = receive(stream).await {
        if tag == b'X' {
            break;
        }
    }
    trace.lock().unwrap().push("DISCONNECTED".into());
}

// This peer does not execute SQL. It validates messages and simulates responses.
async fn protocol_peer(
    listener: TcpListener,
    server: Arc<ServerConfig>,
    behavior: Behavior,
    trace: Trace,
) {
    let mut stream = handshake(&listener, server).await.unwrap();
    authenticate(&mut stream).await;
    let mut value = 0i64;
    while let Some((tag, body)) = receive(&mut stream).await {
        match tag {
            b'Q' => {
                let sql = std::str::from_utf8(&body[..body.len() - 1]).unwrap();
                trace.lock().unwrap().push(sql.into());
                let (command, status) = match sql {
                    "START TRANSACTION ISOLATION LEVEL SERIALIZABLE" => {
                        if matches!(behavior, Behavior::BeginHang) {
                            wait_for_disconnect(&mut stream, &trace).await;
                            return;
                        }
                        (b"BEGIN\0".as_slice(), b"T".as_slice())
                    }
                    "ROLLBACK" => (b"ROLLBACK\0".as_slice(), b"I".as_slice()),
                    "COMMIT" => {
                        match behavior {
                            Behavior::LostCommitReply => return,
                            Behavior::CommitHang => {
                                wait_for_disconnect(&mut stream, &trace).await;
                                return;
                            }
                            Behavior::CommitError(code) => {
                                message(
                                    &mut stream,
                                    b'E',
                                    format!("SERROR\0C{code}\0Msynthetic commit error\0\0")
                                        .as_bytes(),
                                )
                                .await;
                                message(&mut stream, b'Z', b"I").await;
                                continue;
                            }
                            _ => {}
                        }
                        (b"COMMIT\0".as_slice(), b"I".as_slice())
                    }
                    _ => panic!("unexpected query {sql}"),
                };
                message(&mut stream, b'C', command).await;
                message(&mut stream, b'Z', status).await;
            }
            b'P' => {
                let mut body = body.as_slice();
                assert_eq!(cstring(&mut body), b"");
                assert_eq!(cstring(&mut body), b"SELECT $1::INT8");
                assert_eq!(i16::from_be_bytes(take(&mut body)), 1);
                assert_eq!(u32::from_be_bytes(take(&mut body)), 20); // INT8 OID
                assert!(body.is_empty());
                trace.lock().unwrap().push("PARSE typed INT8".into());
                message(&mut stream, b'1', &[]).await;
            }
            b'B' => {
                let mut body = body.as_slice();
                assert_eq!(cstring(&mut body), b"");
                assert_eq!(cstring(&mut body), b"");
                assert_eq!(i16::from_be_bytes(take(&mut body)), 1); // format count
                assert_eq!(i16::from_be_bytes(take(&mut body)), 1); // binary
                assert_eq!(i16::from_be_bytes(take(&mut body)), 1); // param count
                assert_eq!(i32::from_be_bytes(take(&mut body)), 8);
                value = i64::from_be_bytes(take(&mut body));
                assert_eq!(i16::from_be_bytes(take(&mut body)), 1);
                assert_eq!(i16::from_be_bytes(take(&mut body)), 1);
                assert!(body.is_empty());
                trace.lock().unwrap().push(format!("BIND {value}"));
                message(&mut stream, b'2', &[]).await;
            }
            b'D' => {
                assert_eq!(body, b"S\0");
                message(
                    &mut stream,
                    b't',
                    &[
                        1i16.to_be_bytes().as_slice(),
                        20u32.to_be_bytes().as_slice(),
                    ]
                    .concat(),
                )
                .await;
                let row = [
                    1i16.to_be_bytes().as_slice(),
                    b"value\0",
                    &0u32.to_be_bytes(),
                    &0i16.to_be_bytes(),
                    &20u32.to_be_bytes(),
                    &8i16.to_be_bytes(),
                    &(-1i32).to_be_bytes(),
                    &0i16.to_be_bytes(),
                ]
                .concat();
                message(&mut stream, b'T', &row).await;
            }
            b'E' => {
                assert_eq!(body, [0, 0, 0, 0, 0]);
                if matches!(behavior, Behavior::QueryHang) {
                    wait_for_disconnect(&mut stream, &trace).await;
                    return;
                }
                message(
                    &mut stream,
                    b'D',
                    &[
                        1i16.to_be_bytes().as_slice(),
                        &8i32.to_be_bytes(),
                        &value.to_be_bytes(),
                    ]
                    .concat(),
                )
                .await;
                message(&mut stream, b'C', b"SELECT 1\0").await;
            }
            b'S' => {
                assert!(body.is_empty());
                message(&mut stream, b'Z', b"T").await;
            }
            b'X' => break,
            _ => panic!("unexpected frontend tag {tag}"),
        }
    }
    trace.lock().unwrap().push("DISCONNECTED".into());
}

async fn start_peer(behavior: Behavior) -> (Session, tokio::task::JoinHandle<()>, Trace) {
    let f = fixture("127.0.0.1", false);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let trace = Trace::default();
    let peer = tokio::spawn(protocol_peer(listener, f.server, behavior, trace.clone()));
    let session = Session::open(
        &settings(port),
        connector(Trust::PemOnly(&f.root_pem)).unwrap(),
    )
    .await
    .unwrap();
    (session, peer, trace)
}

#[tokio::test(flavor = "current_thread")]
async fn serializable_typed_parameters_and_acknowledged_commit() {
    timeout(BOUND, async {
        let (session, peer, trace) = start_peer(Behavior::Normal).await;
        assert!(matches!(
            tx::round_trip(session, -9223372036854775000, BOUND, BOUND).await,
            ProbeOutcome::Committed(-9223372036854775000)
        ));
        peer.await.unwrap();
        assert_eq!(
            *trace.lock().unwrap(),
            [
                "START TRANSACTION ISOLATION LEVEL SERIALIZABLE",
                "PARSE typed INT8",
                "BIND -9223372036854775000",
                "COMMIT",
                "DISCONNECTED"
            ]
        );
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_and_drop_rollback_are_observed_before_next_transaction() {
    timeout(BOUND, async {
        let (mut session, peer, trace) = start_peer(Behavior::Normal).await;
        let tx = session
            .client
            .build_transaction()
            .isolation_level(IsolationLevel::Serializable)
            .start()
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        let tx = session
            .client
            .build_transaction()
            .isolation_level(IsolationLevel::Serializable)
            .start()
            .await
            .unwrap();
        drop(tx);
        // Waiting for this start proves the prior rollback was processed by the
        // synthetic peer. Production pooling still requires real-server proof.
        session
            .client
            .build_transaction()
            .isolation_level(IsolationLevel::Serializable)
            .start()
            .await
            .unwrap()
            .rollback()
            .await
            .unwrap();
        session.discard().await;
        peer.await.unwrap();
        assert_eq!(
            trace
                .lock()
                .unwrap()
                .iter()
                .filter(|s| *s == "ROLLBACK")
                .count(),
            3
        );
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn lost_commit_reply_and_commit_deadline_are_unknown_and_discarded() {
    timeout(BOUND, async {
        for behavior in [Behavior::LostCommitReply, Behavior::CommitHang] {
            let (session, peer, trace) = start_peer(behavior).await;
            assert!(matches!(
                tx::round_trip(session, 7, BOUND, Duration::from_millis(80)).await,
                ProbeOutcome::Commit(CommitOutcome::Unknown(_))
            ));
            peer.await.unwrap();
            assert!(trace.lock().unwrap().iter().any(|s| s == "COMMIT"));
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn commit_sqlstate_abort_is_distinct_from_other_errors() {
    timeout(BOUND, async {
        for (code, retryable) in [("40001", true), ("40P01", true), ("08006", false)] {
            let (session, peer, _) = start_peer(Behavior::CommitError(code)).await;
            let outcome = tx::round_trip(session, 7, BOUND, BOUND).await;
            assert_eq!(
                matches!(
                    outcome,
                    ProbeOutcome::Commit(CommitOutcome::RetryableAbort(_))
                ),
                retryable
            );
            if !retryable {
                assert!(matches!(
                    outcome,
                    ProbeOutcome::Commit(CommitOutcome::Unknown(Some(_)))
                ));
            }
            peer.await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_begin_or_query_discards_connection_without_commit() {
    timeout(BOUND, async {
        for behavior in [Behavior::BeginHang, Behavior::QueryHang] {
            let (session, peer, trace) = start_peer(behavior).await;
            assert!(matches!(
                tx::round_trip(session, 7, Duration::from_millis(80), BOUND).await,
                ProbeOutcome::BeforeCommitDeadline
            ));
            peer.await.unwrap();
            assert!(!trace.lock().unwrap().iter().any(|s| s == "COMMIT"));
            assert!(trace.lock().unwrap().iter().any(|s| s == "DISCONNECTED"));
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_session_aborts_driver_instead_of_detaching() {
    timeout(BOUND, async {
        let (session, peer, trace) = start_peer(Behavior::Normal).await;
        drop(session);
        peer.await.unwrap();
        assert_eq!(*trace.lock().unwrap(), ["DISCONNECTED"]);
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn cancel_request_uses_same_verified_tls_and_synthetic_backend_key() {
    timeout(BOUND, async {
        let f = fixture("127.0.0.1", false);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let tls = connector(Trust::PemOnly(&f.root_pem)).unwrap();
        let peer = tokio::spawn(async move {
            let mut stream = handshake(&listener, f.server.clone()).await.unwrap();
            authenticate(&mut stream).await;
            let mut cancel = handshake(&listener, f.server).await.unwrap();
            let mut request = [0; 16];
            cancel.read_exact(&mut request).await.unwrap();
            assert_eq!(
                request,
                [
                    16u32.to_be_bytes(),
                    80877102u32.to_be_bytes(),
                    123u32.to_be_bytes(),
                    456u32.to_be_bytes()
                ]
                .concat()
                .as_slice()
            );
            drop(cancel);
            assert!(receive(&mut stream).await.is_none());
        });
        let session = Session::open(&settings(port), tls.clone()).await.unwrap();
        session
            .client
            .cancel_token()
            .cancel_query(tls)
            .await
            .unwrap();
        session.discard().await;
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn expired_intermediate_ca_is_rejected() {
    let mut root = CertificateParams::new(Vec::<String>::new()).unwrap();
    root.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    root.distinguished_name
        .push(DnType::CommonName, "root-for-expired-intermediate");
    root.key_usages = vec![KeyUsagePurpose::KeyCertSign];
    let root_key = KeyPair::generate().unwrap();
    let root_cert = root.self_signed(&root_key).unwrap();
    let root_issuer = Issuer::new(root, root_key);
    let mut intermediate = CertificateParams::new(Vec::<String>::new()).unwrap();
    intermediate.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    intermediate
        .distinguished_name
        .push(DnType::CommonName, "expired-intermediate");
    intermediate.key_usages = vec![KeyUsagePurpose::KeyCertSign];
    intermediate.not_before = date_time_ymd(2000, 1, 1);
    intermediate.not_after = date_time_ymd(2001, 1, 1);
    let intermediate_key = KeyPair::generate().unwrap();
    let intermediate_cert = intermediate
        .signed_by(&intermediate_key, &root_issuer)
        .unwrap();
    let intermediate_issuer = Issuer::new(intermediate, intermediate_key);
    let mut leaf = CertificateParams::new(vec!["127.0.0.1".into()]).unwrap();
    leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let key = KeyPair::generate().unwrap();
    let cert = leaf.signed_by(&key, &intermediate_issuer).unwrap();
    let server =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone(), intermediate_cert.der().clone()],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .unwrap();
    let pem = root_cert.pem().into_bytes();
    let (error, reached) = tls_probe(
        Fixture {
            root_pem: pem.clone(),
            server: Arc::new(server),
        },
        Some(pem),
    )
    .await;
    assert!(!reached);
    assert!(
        error_chain(&error).contains("expired"),
        "{}",
        error_chain(&error)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn cancel_request_cannot_fall_back_to_plaintext_or_unrelated_ca() {
    timeout(BOUND, async {
        for plaintext in [true, false] {
            let f = fixture("127.0.0.1", false);
            let unrelated = fixture("127.0.0.1", false);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let tls = connector(Trust::PemOnly(&f.root_pem)).unwrap();
            let peer = tokio::spawn(async move {
                let mut stream = handshake(&listener, f.server).await.unwrap();
                authenticate(&mut stream).await;
                if plaintext {
                    let (mut cancel, _) = listener.accept().await.unwrap();
                    let mut request = [0; 8];
                    cancel.read_exact(&mut request).await.unwrap();
                    assert_eq!(request, SSL_REQUEST);
                    cancel.write_all(b"N").await.unwrap();
                    assert_eq!(cancel.read(&mut [0; 1]).await.unwrap(), 0);
                } else if let Some(mut cancel) = handshake(&listener, unrelated.server).await {
                    // TLS 1.3 may finish server-side before the validation alert.
                    assert!(cancel.read_u32().await.is_err());
                }
                assert!(receive(&mut stream).await.is_none());
            });
            let session = Session::open(&settings(port), tls.clone()).await.unwrap();
            let error = session
                .client
                .cancel_token()
                .cancel_query(tls)
                .await
                .unwrap_err();
            assert!(error_chain(&error).contains(if plaintext {
                "server does not support TLS"
            } else {
                "UnknownIssuer"
            }));
            session.discard().await;
            peer.await.unwrap();
        }
    })
    .await
    .unwrap();
}
