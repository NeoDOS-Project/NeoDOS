//! Protocol constants, errors, result and transport types.

use alloc::vec::Vec;

// ── Protocol constants ──

pub const DNS_PORT: u16 = 53;
pub const DNS_TYPE_A: u16 = 1;
pub const DNS_TYPE_CNAME: u16 = 5;
pub const DNS_CLASS_IN: u16 = 1;

/// CNAME chain hop limit, to protect against loops.
pub const DNS_MAX_CNAME_HOPS: usize = 10;

// ── Errors ──

/// Distinct DNS failures. Callers present these individually rather than
/// collapsing everything into a generic "DNS failed".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsError {
    /// The network configuration could not be read (Registry unavailable).
    NoConfig,
    /// No DNS server is configured.
    NoServer,
    /// The hostname is malformed (empty label, illegal character, too long, ...).
    InvalidHostname,
    /// No response arrived before the deadline.
    Timeout,
    /// The server could not be reached (socket setup/send failed).
    ServerUnreachable,
    /// The server answered with RCODE=3 (NXDOMAIN).
    NxDomain,
    /// The response was not a well-formed DNS reply.
    MalformedResponse,
    /// The response had the TC (truncated) bit set — TCP fallback is unsupported.
    Truncated,
    /// The server answered with a non-zero RCODE other than NXDOMAIN.
    ServerFailure,
    /// The response was valid but contained no A record for the name.
    NoARecord,
    /// The network transport (net.nxl / sockets) is unavailable.
    Network,
}

// ── Results ──

/// Result of parsing a single response (one server, one query).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsAnswer {
    pub addresses: Vec<[u8; 4]>,
    pub ttl: u32,
}

impl DnsAnswer {
    pub fn first(&self) -> Option<[u8; 4]> {
        self.addresses.first().copied()
    }
}

// ── Transport abstraction ──

/// DNS datagram transport. Abstracting this lets the resolver be unit tested
/// with synthetic responses and no network access.
pub trait DnsTransport {
    /// Send `query` to `server:53` and copy the reply into `resp`.
    /// Returns the reply length, or a [`DnsError`] on failure.
    fn exchange(
        &mut self,
        server: [u8; 4],
        query: &[u8],
        resp: &mut [u8],
    ) -> Result<usize, DnsError>;
}
