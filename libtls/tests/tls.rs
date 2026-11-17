//! End-to-end TLS 1.3 handshake against a real `openssl s_server`.
//!
//! The test PKI is generated at run time (nothing secret is committed): a CA,
//! and a leaf for `localhost` with a DNS SAN. `openssl` must be on PATH.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use libtls::{ClientConfig, Host, Rng, TlsError, Transport};

struct StdTransport(TcpStream);

impl Transport for StdTransport {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, TlsError> {
        self.0.read(buf).map_err(|_| TlsError::Io)
    }

    fn write_all(&mut self, data: &[u8]) -> Result<(), TlsError> {
        self.0.write_all(data).map_err(|_| TlsError::Io)
    }

    fn close(&mut self) {
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }
}

struct OsRng;

impl Rng for OsRng {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), TlsError> {
        let mut f = std::fs::File::open("/dev/urandom").map_err(|_| TlsError::RngUnavailable)?;
        f.read_exact(buf).map_err(|_| TlsError::RngUnavailable)
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

struct FixedClock(i64);

impl libclock::Clock for FixedClock {
    fn now_unix(&self) -> Option<i64> {
        Some(self.0)
    }
}

fn openssl(args: &[&str]) {
    let status = Command::new("openssl")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("openssl");
    assert!(status.success(), "openssl {args:?} failed");
}

struct Fixture {
    child: Child,
    port: u16,
    roots: Vec<Vec<u8>>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Each test gets its own PKI: the tests run in parallel and used to share a
/// directory keyed only by the process id, so they clobbered each other's CA.
static FIXTURE_ID: AtomicU32 = AtomicU32::new(0);

fn start_server() -> Option<Fixture> {
    let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "neodos-libtls-{}-{id}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    let p = |name: &str| dir.join(name).to_string_lossy().into_owned();

    openssl(&[
        "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", &p("ca.key"),
        "-out", &p("ca.pem"), "-days", "3650", "-subj", "/CN=NeoDOS Test CA",
    ]);
    openssl(&[
        "req", "-newkey", "rsa:2048", "-nodes", "-keyout", &p("leaf.key"),
        "-out", &p("leaf.csr"), "-subj", "/CN=localhost",
    ]);
    std::fs::write(
        p("leaf.ext"),
        "subjectAltName=DNS:localhost,IP:10.0.2.2\nbasicConstraints=CA:FALSE\n\
         extendedKeyUsage=serverAuth\n",
    )
    .ok()?;
    openssl(&[
        "x509", "-req", "-in", &p("leaf.csr"), "-CA", &p("ca.pem"), "-CAkey", &p("ca.key"),
        "-CAcreateserial", "-out", &p("leaf.pem"), "-days", "365", "-extfile", &p("leaf.ext"),
    ]);
    openssl(&["x509", "-in", &p("ca.pem"), "-outform", "DER", "-out", &p("ca.der")]);
    let roots = vec![std::fs::read(p("ca.der")).ok()?];

    // Pick a free port.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    drop(listener);

    let child = Command::new("openssl")
        .args([
            "s_server", "-tls1_3", "-accept", &format!("127.0.0.1:{port}"),
            "-cert", &p("leaf.pem"), "-key", &p("leaf.key"), "-www", "-quiet",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Some(Fixture { child, port, roots });
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

#[test]
fn tls13_handshake_against_openssl() {
    let Some(fixture) = start_server() else {
        eprintln!("skipping: openssl s_server unavailable");
        return;
    };
    let stream = TcpStream::connect(("127.0.0.1", fixture.port)).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    stream.set_write_timeout(Some(Duration::from_secs(10))).unwrap();

    let mut rng = OsRng;
    let clock = FixedClock(now());
    let mut tls = libtls::connect(
        StdTransport(stream),
        ClientConfig {
            roots: &fixture.roots,
            clock: &clock,
            rng: &mut rng,
            server_name: Some("localhost"),
            host: Host::Dns("localhost"),
        },
    )
    .expect("TLS handshake");

    tls.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("write request");
    let mut response = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        match tls.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => response.extend_from_slice(&buf[..n]),
            Err(e) => panic!("read failed: {e:?}"),
        }
        if response.len() > 64 * 1024 {
            break;
        }
    }
    let text = String::from_utf8_lossy(&response);
    assert!(text.starts_with("HTTP/"), "unexpected response: {text:?}");
}

#[test]
fn tls13_rejects_wrong_hostname() {
    let Some(fixture) = start_server() else {
        eprintln!("skipping: openssl s_server unavailable");
        return;
    };
    let stream = TcpStream::connect(("127.0.0.1", fixture.port)).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut rng = OsRng;
    let clock = FixedClock(now());
    let result = libtls::connect(
        StdTransport(stream),
        ClientConfig {
            roots: &fixture.roots,
            clock: &clock,
            rng: &mut rng,
            server_name: Some("wrong.example"),
            host: Host::Dns("wrong.example"),
        },
    );
    match result {
        Err(TlsError::HostnameMismatch) | Err(TlsError::Alert) | Err(TlsError::Closed) => {}
        Ok(_) => panic!("handshake must fail for a wrong hostname"),
        Err(e) => panic!("unexpected error {e:?}"),
    }
}

#[test]
fn tls13_rejects_untrusted_issuer() {
    let Some(fixture) = start_server() else { return };
    let stream = TcpStream::connect(("127.0.0.1", fixture.port)).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut rng = OsRng;
    let clock = FixedClock(now());
    let result = libtls::connect(
        StdTransport(stream),
        ClientConfig {
            roots: &[],
            clock: &clock,
            rng: &mut rng,
            server_name: Some("localhost"),
            host: Host::Dns("localhost"),
        },
    );
    assert!(matches!(result, Err(TlsError::NotTrusted)));
}

#[test]
fn tls13_rejects_expired_and_not_yet_valid() {
    let Some(fixture) = start_server() else { return };
    for (offset, want) in [
        (3 * 365 * 86_400i64, TlsError::Expired),
        (-3 * 365 * 86_400i64, TlsError::NotYetValid),
    ] {
        let stream = TcpStream::connect(("127.0.0.1", fixture.port)).expect("connect");
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut rng = OsRng;
        let clock = FixedClock(now() + offset);
        let result = libtls::connect(
            StdTransport(stream),
            ClientConfig {
                roots: &fixture.roots,
                clock: &clock,
                rng: &mut rng,
                server_name: Some("localhost"),
                host: Host::Dns("localhost"),
            },
        );
        assert!(matches!(result, Err(e) if e == want), "offset {offset}: got ");
    }
}

#[test]
fn tls13_accepts_ip_san_without_sni() {
    let Some(fixture) = start_server() else { return };
    let stream = TcpStream::connect(("127.0.0.1", fixture.port)).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut rng = OsRng;
    let clock = FixedClock(now());
    let result = libtls::connect(
        StdTransport(stream),
        ClientConfig {
            roots: &fixture.roots,
            clock: &clock,
            rng: &mut rng,
            server_name: None,
            host: Host::Ip([10, 0, 2, 2]),
        },
    );
    assert!(result.is_ok(), "IP SAN handshake failed");
}
