//! Manual, network-dependent check against a real TLS 1.3 server.
//!
//! Nothing is committed: the trust anchor is read from `TLS_TEST_ROOT` (DER).
//!
//! ```text
//! curl -sS -o /tmp/isrgx1.der https://letsencrypt.org/certs/isrgrootx1.der
//! TLS_TEST_ROOT=/tmp/isrgx1.der cargo test --test real -- --ignored --nocapture
//! ```

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

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

struct SystemClock;

impl libclock::Clock for SystemClock {
    fn now_unix(&self) -> Option<i64> {
        Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs() as i64,
        )
    }
}

#[test]
#[ignore = "requires network access and TLS_TEST_ROOT"]
fn real_server_handshake() {
    let root_path = std::env::var("TLS_TEST_ROOT").expect("TLS_TEST_ROOT (DER)");
    let roots = vec![std::fs::read(root_path).expect("read root")];
    let host = std::env::var("TLS_TEST_HOST")
        .unwrap_or_else(|_| "valid-isrgrootx1.letsencrypt.org".into());
    let path = std::env::var("TLS_TEST_PATH").unwrap_or_else(|_| "/".into());

    let stream = TcpStream::connect((host.as_str(), 443)).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    stream.set_write_timeout(Some(Duration::from_secs(15))).unwrap();

    let mut rng = OsRng;
    let clock = SystemClock;
    let mut tls = libtls::connect(
        StdTransport(stream),
        ClientConfig {
            roots: &roots,
            clock: &clock,
            rng: &mut rng,
            server_name: Some(&host),
            host: Host::Dns(&host),
        },
    )
    .expect("TLS handshake");

    let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    tls.write_all(request.as_bytes()).expect("write");
    let mut response = Vec::new();
    let mut buf = [0u8; 2048];
    loop {
        match tls.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => response.extend_from_slice(&buf[..n]),
            Err(e) => panic!("read: {e:?}"),
        }
        if response.len() > 8 * 1024 * 1024 {
            break;
        }
    }
    let text = String::from_utf8_lossy(&response);
    println!("--- response head ---\n{}", &text[..text.len().min(200)]);
    println!("--- total bytes: {} ---", response.len());
    assert!(text.starts_with("HTTP/"), "unexpected response");
    assert!(text.contains(" 200 ") || text.contains(" 301 "), "status");
}
