//! SQLx driver feasibility probes, deliberately outside the production workspace.
//!
//! The loopback peer implements only SSLRequest and a startup rejection. It is
//! not PostgreSQL and provides no persistence, transaction, or parity evidence.
#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use std::{
        process::Command,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };

    use rcgen::{
        date_time_ymd, BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa,
        Issuer, KeyPair, KeyUsagePurpose,
    };
    use rustls::{pki_types::PrivatePkcs8KeyDer, ServerConfig};
    use sqlx::{postgres::PgSslMode, ConnectOptions, Error, PgConnection};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        time::timeout,
    };
    use tokio_rustls::TlsAcceptor;

    const SSL_REQUEST: [u8; 8] = [0, 0, 0, 8, 4, 210, 22, 47];
    const STARTUP_REACHED: &str = "TLS_PROBE_REACHED_STARTUP";
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

    fn options(port: u16) -> sqlx::postgres::PgConnectOptions {
        sqlx::postgres::PgConnectOptions::new_without_pgpass()
            .host("127.0.0.1")
            .port(port)
            .username("synthetic-probe")
            .password("")
            .database("synthetic-probe")
            .application_name("ledgerlab-driver-gate")
            .ssl_mode(PgSslMode::VerifyFull)
            .disable_statement_logging()
    }

    // Return whether PostgreSQL startup bytes crossed the verified TLS channel.
    // Errors in the test peer itself are separate from expected client rejection.
    async fn peer(listener: TcpListener, server: Arc<ServerConfig>) -> bool {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 8];
        stream.read_exact(&mut request).await.unwrap();
        assert_eq!(request, SSL_REQUEST);
        stream.write_all(b"S").await.unwrap();
        let Ok(mut tls) = TlsAcceptor::from(server).accept(stream).await else {
            return false;
        };
        // TLS 1.3 may finish on the server before the client rejects its cert.
        let Ok(length) = tls.read_u32().await else {
            return false;
        };
        assert!((8..=8192).contains(&length));
        let mut startup = vec![0; length as usize - 4];
        tls.read_exact(&mut startup).await.unwrap();
        assert_eq!(&startup[..4], &[0, 3, 0, 0]);
        let fields = format!("SFATAL\0C08004\0M{STARTUP_REACHED}\0\0");
        tls.write_u8(b'E').await.unwrap();
        tls.write_u32((fields.len() + 4) as u32).await.unwrap();
        tls.write_all(fields.as_bytes()).await.unwrap();
        tls.flush().await.unwrap();
        true
    }

    async fn probe(fixture: Fixture, trust: Option<Vec<u8>>) -> (Error, bool) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut opts = options(listener.local_addr().unwrap().port());
        if let Some(pem) = trust {
            opts = opts.ssl_root_cert_from_pem(pem);
        }
        let (client, server) = timeout(BOUND, async {
            tokio::join!(opts.connect(), peer(listener, fixture.server))
        })
        .await
        .expect("bounded local TLS probe timed out");
        (client.unwrap_err(), server)
    }

    fn assert_tls_rejected(error: Error, startup: bool, reason: &str) {
        assert!(
            !startup,
            "startup leaked through failed certificate validation"
        );
        assert!(matches!(error, Error::Tls(_) | Error::Io(_)), "{error}");
        assert!(error.to_string().contains(reason), "{error}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn supplied_private_ca_reaches_startup() {
        let f = fixture("127.0.0.1", false);
        let pem = f.root_pem.clone();
        let (error, startup) = probe(f, Some(pem)).await;
        assert!(startup);
        let Error::Database(db) = error else {
            panic!("expected the test peer's explicit startup rejection")
        };
        assert_eq!(db.code().as_deref(), Some("08004"));
        assert_eq!(db.message(), STARTUP_REACHED);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn wrong_hostname_rejected() {
        let f = fixture("wrong-host.invalid", false);
        let pem = f.root_pem.clone();
        let (error, startup) = probe(f, Some(pem)).await;
        assert_tls_rejected(error, startup, "certificate not valid for name");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn expired_certificate_rejected() {
        let f = fixture("127.0.0.1", true);
        let pem = f.root_pem.clone();
        let (error, startup) = probe(f, Some(pem)).await;
        assert_tls_rejected(error, startup, "certificate expired");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unrelated_private_ca_rejected() {
        let f = fixture("127.0.0.1", false);
        let other = fixture("127.0.0.1", false);
        let (error, startup) = probe(f, Some(other.root_pem)).await;
        assert_tls_rejected(error, startup, "UnknownIssuer");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bundled_public_roots_do_not_trust_generated_private_ca() {
        let (error, startup) = probe(fixture("127.0.0.1", false), None).await;
        assert_tls_rejected(error, startup, "UnknownIssuer");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn verify_full_rejects_plaintext_without_startup_fallback() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let opts = options(listener.local_addr().unwrap().port());
        let server = async {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 8];
            stream.read_exact(&mut request).await.unwrap();
            assert_eq!(request, SSL_REQUEST);
            stream.write_all(b"N").await.unwrap();
            let mut trailing = [0; 1];
            assert_eq!(stream.read(&mut trailing).await.unwrap(), 0);
        };
        let (client, ()) = timeout(BOUND, async { tokio::join!(opts.connect(), server) })
            .await
            .expect("bounded plaintext probe timed out");
        assert!(matches!(client, Err(Error::Tls(_))));
    }

    #[test]
    fn new_without_pgpass_still_imports_ambient_options() {
        // Use a child so the test never mutates this multithread-capable process's
        // environment. The variables are synthetic and never contain secrets.
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::ambient_child", "--ignored"])
            .env_clear()
            .env("PGHOST", "ambient.invalid")
            .env("PGPORT", "5439")
            .env("PGUSER", "ambient-user")
            .env("PGDATABASE", "ambient-db")
            .env("PGSSLMODE", "disable")
            .env("PGOPTIONS", "-c synchronous_commit=off")
            .env("PGSSLROOTCERT", "/synthetic/ambient-root.pem")
            .env("PGSSLCERT", "/synthetic/ambient-client.pem")
            .env("PGSSLKEY", "/synthetic/ambient-key.pem")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "ambient options reproduction failed"
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }

    #[test]
    #[ignore = "invoked by parent in a clean subprocess with synthetic PG variables"]
    fn ambient_child() {
        let opts = sqlx::postgres::PgConnectOptions::new_without_pgpass();
        assert_eq!(opts.get_host(), "ambient.invalid");
        assert_eq!(opts.get_port(), 5439);
        assert_eq!(opts.get_username(), "ambient-user");
        assert_eq!(opts.get_database(), Some("ambient-db"));
        assert!(matches!(opts.get_ssl_mode(), PgSslMode::Disable));
        assert_eq!(opts.get_options(), Some("-c synchronous_commit=off"));
        // No public getters/resetters exist for these three options. Examine the
        // synthetic-only object's Debug locally, without printing credentials.
        let debug = format!("{opts:?}");
        assert!(debug.contains("/synthetic/ambient-root.pem"));
        assert!(debug.contains("/synthetic/ambient-client.pem"));
        assert!(debug.contains("/synthetic/ambient-key.pem"));
    }

    // Type-level feasibility only: no claim this ran against a real server.
    // The future borrows the connection until the tracked guard is ended.
    #[allow(dead_code)]
    async fn tracked_serializable_signature(connection: &mut PgConnection) -> Result<(), Error> {
        use sqlx::Connection;
        let transaction = connection
            .begin_with("BEGIN ISOLATION LEVEL SERIALIZABLE")
            .await?;
        transaction.rollback().await
    }
}
