//! Query execution against one or more servers.

use crate::*;

// ── Query execution ──

/// Global next DNS query ID.
static NEXT_DNS_ID: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(1);

fn next_id() -> u16 {
    let id = NEXT_DNS_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    if id == 0 {
        NEXT_DNS_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
    } else {
        id
    }
}

/// Build a query, exchange it with `server` and parse the reply.
pub fn query_server<T: DnsTransport>(
    transport: &mut T,
    hostname: &str,
    server: [u8; 4],
    id: u16,
) -> Result<DnsAnswer, DnsError> {
    let query = build_query(hostname, id);
    let mut buf = [0u8; 512];
    let len = transport.exchange(server, &query, &mut buf)?;
    parse_response(&buf[..len], id)
}

/// Query one server, retrying transient failures (timeout / unreachable).
pub fn query_server_with_retries<T: DnsTransport>(
    transport: &mut T,
    hostname: &str,
    server: [u8; 4],
    retries: usize,
) -> Result<DnsAnswer, DnsError> {
    let mut last = DnsError::Timeout;
    for _ in 0..=retries {
        match query_server(transport, hostname, server, next_id()) {
            Ok(answer) => return Ok(answer),
            Err(err @ (DnsError::Timeout | DnsError::ServerUnreachable)) => last = err,
            Err(err) => return Err(err),
        }
    }
    Err(last)
}

/// Resolve `hostname` against a list of servers, in preference order.
///
/// `NXDOMAIN` from any server is returned immediately. Unspecified (`0.0.0.0`)
/// servers are skipped. On success returns the answer and the server that
/// produced it.
pub fn resolve_with_servers<T: DnsTransport>(
    transport: &mut T,
    hostname: &str,
    servers: &[[u8; 4]],
    retries: usize,
) -> Result<(DnsAnswer, [u8; 4]), DnsError> {
    validate_hostname(hostname)?;

    let mut last = DnsError::NoServer;
    let mut tried = false;

    for &server in servers {
        if is_unspecified(server) {
            continue;
        }
        tried = true;
        match query_server_with_retries(transport, hostname, server, retries) {
            Ok(answer) => return Ok((answer, server)),
            Err(DnsError::NxDomain) => return Err(DnsError::NxDomain),
            Err(err) => last = err,
        }
    }

    if !tried {
        return Err(DnsError::NoServer);
    }
    Err(last)
}
