    use super::*;
    use alloc::vec;

    // ── Test doubles ──

    struct MockTransport {
        response: Vec<u8>,
        last_server: Option<[u8; 4]>,
        calls: usize,
    }

    impl MockTransport {
        fn new(response: Vec<u8>) -> Self {
            MockTransport {
                response,
                last_server: None,
                calls: 0,
            }
        }
    }

    impl DnsTransport for MockTransport {
        fn exchange(
            &mut self,
            server: [u8; 4],
            _query: &[u8],
            resp: &mut [u8],
        ) -> Result<usize, DnsError> {
            self.calls += 1;
            self.last_server = Some(server);
            let len = self.response.len().min(resp.len());
            resp[..len].copy_from_slice(&self.response[..len]);
            Ok(len)
        }
    }

    struct FailingTransport {
        error: DnsError,
    }

    impl DnsTransport for FailingTransport {
        fn exchange(
            &mut self,
            _server: [u8; 4],
            _query: &[u8],
            _resp: &mut [u8],
        ) -> Result<usize, DnsError> {
            Err(self.error)
        }
    }

    fn push_u16(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_be_bytes());
    }

    fn push_u32(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_be_bytes());
    }

    /// Build a synthetic response: question + one A record per address.
    fn response_with_a(
        name: &str,
        addrs: &[[u8; 4]],
        id: u16,
        rcode: u8,
        truncated: bool,
    ) -> Vec<u8> {
        let mut pkt = Vec::new();
        let flags = 0x8000u16 | (if truncated { 0x0200 } else { 0 }) | (rcode as u16);
        push_u16(&mut pkt, id);
        push_u16(&mut pkt, flags);
        push_u16(&mut pkt, 1); // QDCOUNT
        push_u16(&mut pkt, addrs.len() as u16); // ANCOUNT
        push_u16(&mut pkt, 0);
        push_u16(&mut pkt, 0);

        pkt.extend_from_slice(&encode_name(name));
        push_u16(&mut pkt, DNS_TYPE_A);
        push_u16(&mut pkt, DNS_CLASS_IN);

        for addr in addrs {
            push_u16(&mut pkt, 0xC00C); // owner -> question name
            push_u16(&mut pkt, DNS_TYPE_A);
            push_u16(&mut pkt, DNS_CLASS_IN);
            push_u32(&mut pkt, 300);
            push_u16(&mut pkt, 4);
            pkt.extend_from_slice(addr);
        }
        pkt
    }

    // ── Encoding ──

    #[test]
    fn encode_name_labels() {
        assert_eq!(
            encode_name("www.example.com"),
            vec![3, b'w', b'w', b'w', 7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0]
        );
    }

    #[test]
    fn encode_name_handles_trailing_dot() {
        assert_eq!(encode_name("example.com."), encode_name("example.com"));
    }

    #[test]
    fn decode_name_roundtrip() {
        let wire = encode_name("www.example.com");
        let (name, next) = decode_name(&wire, 0).unwrap();
        assert_eq!(name, "www.example.com");
        assert_eq!(next, wire.len());
    }

    #[test]
    fn decode_name_follows_pointer() {
        let mut data = encode_name("example.com");
        data.push(0xC0);
        data.push(0x00);
        let (name, next) = decode_name(&data, 13).unwrap();
        assert_eq!(name, "example.com");
        assert_eq!(next, 15);
    }

    // ── Query construction ──

    #[test]
    fn build_query_header_and_question() {
        let q = build_query("example.com", 0x1234);
        assert!(q.len() > 12);
        assert_eq!(u16::from_be_bytes([q[0], q[1]]), 0x1234);
        assert_eq!(u16::from_be_bytes([q[2], q[3]]), 0x0100); // RD
        assert_eq!(u16::from_be_bytes([q[4], q[5]]), 1); // QDCOUNT
        assert_eq!(u16::from_be_bytes([q[6], q[7]]), 0); // ANCOUNT
        let n = q.len();
        assert_eq!(u16::from_be_bytes([q[n - 4], q[n - 3]]), DNS_TYPE_A);
        assert_eq!(u16::from_be_bytes([q[n - 2], q[n - 1]]), DNS_CLASS_IN);
        assert_eq!(u16::from_be_bytes([q[2], q[3]]) & 0x8000, 0);
    }

    // ── Response parsing ──

    #[test]
    fn parse_single_a_record() {
        let resp = response_with_a("example.com", &[[93, 184, 216, 34]], 7, 0, false);
        let answer = parse_response(&resp, 7).unwrap();
        assert_eq!(answer.addresses, vec![[93, 184, 216, 34]]);
        assert_eq!(answer.ttl, 300);
    }

    #[test]
    fn parse_multiple_a_records() {
        let addrs = [[1, 2, 3, 4], [5, 6, 7, 8], [9, 10, 11, 12]];
        let resp = response_with_a("multi.example.com", &addrs, 9, 0, false);
        let answer = parse_response(&resp, 9).unwrap();
        assert_eq!(answer.addresses, vec![addrs[0], addrs[1], addrs[2]]);
    }

    #[test]
    fn parse_nxdomain() {
        let resp = response_with_a("nope.example.com", &[], 3, 3, false);
        assert_eq!(parse_response(&resp, 3), Err(DnsError::NxDomain));
    }

    #[test]
    fn parse_server_failure() {
        let resp = response_with_a("fail.example.com", &[], 3, 2, false);
        assert_eq!(parse_response(&resp, 3), Err(DnsError::ServerFailure));
    }

    #[test]
    fn parse_truncated() {
        let resp = response_with_a("big.example.com", &[[1, 2, 3, 4]], 3, 0, true);
        assert_eq!(parse_response(&resp, 3), Err(DnsError::Truncated));
    }

    #[test]
    fn parse_rejects_wrong_id() {
        let resp = response_with_a("example.com", &[[1, 2, 3, 4]], 10, 0, false);
        assert_eq!(parse_response(&resp, 11), Err(DnsError::MalformedResponse));
    }

    #[test]
    fn parse_rejects_short_and_garbage() {
        assert_eq!(parse_response(&[], 1), Err(DnsError::MalformedResponse));
        assert_eq!(parse_response(&[0u8; 5], 1), Err(DnsError::MalformedResponse));
        let mut bad = vec![0, 1, 0x80, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        bad.push(10); // label length 10 with no data
        assert!(parse_response(&bad, 1).is_err());
    }

    #[test]
    fn parse_no_a_record() {
        let resp = response_with_a("empty.example.com", &[], 4, 0, false);
        assert_eq!(parse_response(&resp, 4), Err(DnsError::NoARecord));
    }

    // ── CNAME chain ──

    #[test]
    fn parse_follows_cname_chain() {
        let mut pkt = Vec::new();
        push_u16(&mut pkt, 42);
        push_u16(&mut pkt, 0x8180);
        push_u16(&mut pkt, 1);
        push_u16(&mut pkt, 2);
        push_u16(&mut pkt, 0);
        push_u16(&mut pkt, 0);

        pkt.extend_from_slice(&encode_name("alias.example.com"));
        push_u16(&mut pkt, DNS_TYPE_A);
        push_u16(&mut pkt, DNS_CLASS_IN);

        // Answer 1: alias.example.com CNAME canonical.example.com
        push_u16(&mut pkt, 0xC00C);
        push_u16(&mut pkt, DNS_TYPE_CNAME);
        push_u16(&mut pkt, DNS_CLASS_IN);
        push_u32(&mut pkt, 120);
        let cname = encode_name("canonical.example.com");
        push_u16(&mut pkt, cname.len() as u16);
        pkt.extend_from_slice(&cname);

        // Answer 2: canonical.example.com A 10.20.30.40
        pkt.extend_from_slice(&encode_name("canonical.example.com"));
        push_u16(&mut pkt, DNS_TYPE_A);
        push_u16(&mut pkt, DNS_CLASS_IN);
        push_u32(&mut pkt, 60);
        push_u16(&mut pkt, 4);
        pkt.extend_from_slice(&[10, 20, 30, 40]);

        let answer = parse_response(&pkt, 42).unwrap();
        assert_eq!(answer.addresses, vec![[10, 20, 30, 40]]);
        assert_eq!(answer.ttl, 60);
    }

    // ── Hostname validation ──

    #[test]
    fn validate_hostname_accepts_normal_names() {
        assert!(validate_hostname("example.com").is_ok());
        assert!(validate_hostname("a.b.c.d.example.com").is_ok());
        assert!(validate_hostname("host-1.example.com").is_ok());
        assert!(validate_hostname("example.com.").is_ok());
    }

    #[test]
    fn validate_hostname_rejects_bad_names() {
        assert_eq!(validate_hostname(""), Err(DnsError::InvalidHostname));
        assert_eq!(validate_hostname("."), Err(DnsError::InvalidHostname));
        assert_eq!(validate_hostname("a..b"), Err(DnsError::InvalidHostname));
        assert_eq!(validate_hostname("-bad.example.com"), Err(DnsError::InvalidHostname));
        assert_eq!(validate_hostname("bad-.example.com"), Err(DnsError::InvalidHostname));
        assert_eq!(validate_hostname("has space.com"), Err(DnsError::InvalidHostname));
        let long_label = "a".repeat(64);
        assert_eq!(validate_hostname(&long_label), Err(DnsError::InvalidHostname));
    }

    // ── Server selection / transport ──

    #[test]
    fn query_server_uses_given_server_and_parses() {
        let resp = response_with_a("example.com", &[[8, 8, 4, 4]], 55, 0, false);
        let mut t = MockTransport::new(resp);
        let answer = query_server(&mut t, "example.com", [10, 0, 1, 1], 55).unwrap();
        assert_eq!(answer.addresses, vec![[8, 8, 4, 4]]);
        assert_eq!(t.last_server, Some([10, 0, 1, 1]));
        assert_eq!(t.calls, 1);
    }

    #[test]
    fn resolve_with_servers_picks_working_server() {
        // Response has an arbitrary ID; retries generate IDs, so answer parsing
        // must match. Use a transport that echoes the query ID by inspecting it.
        struct EchoTransport(Vec<u8>);
        impl DnsTransport for EchoTransport {
            fn exchange(
                &mut self,
                _server: [u8; 4],
                query: &[u8],
                resp: &mut [u8],
            ) -> Result<usize, DnsError> {
                let id = u16::from_be_bytes([query[0], query[1]]);
                let mut r = self.0.clone();
                r[0..2].copy_from_slice(&id.to_be_bytes());
                let len = r.len().min(resp.len());
                resp[..len].copy_from_slice(&r[..len]);
                Ok(len)
            }
        }

        let base = response_with_a("example.com", &[[9, 9, 9, 9]], 0, 0, false);
        let mut t = EchoTransport(base);
        let servers = [[192, 168, 1, 1], [10, 0, 0, 1]];
        let (answer, server) = resolve_with_servers(&mut t, "example.com", &servers, 0).unwrap();
        assert_eq!(answer.addresses, vec![[9, 9, 9, 9]]);
        assert_eq!(server, [192, 168, 1, 1]);
    }

    #[test]
    fn resolve_with_servers_skips_bad_server() {
        struct Selective {
            bad: [u8; 4],
            good: Vec<u8>,
        }
        impl DnsTransport for Selective {
            fn exchange(
                &mut self,
                server: [u8; 4],
                query: &[u8],
                resp: &mut [u8],
            ) -> Result<usize, DnsError> {
                if server == self.bad {
                    return Err(DnsError::Timeout);
                }
                let id = u16::from_be_bytes([query[0], query[1]]);
                let mut r = self.good.clone();
                r[0..2].copy_from_slice(&id.to_be_bytes());
                let len = r.len().min(resp.len());
                resp[..len].copy_from_slice(&r[..len]);
                Ok(len)
            }
        }

        let mut t = Selective {
            bad: [192, 168, 1, 1],
            good: response_with_a("example.com", &[[7, 7, 7, 7]], 0, 0, false),
        };
        let servers = [[192, 168, 1, 1], [10, 0, 0, 1]];
        let (answer, server) = resolve_with_servers(&mut t, "example.com", &servers, 0).unwrap();
        assert_eq!(answer.addresses, vec![[7, 7, 7, 7]]);
        assert_eq!(server, [10, 0, 0, 1]);
    }

    #[test]
    fn resolve_with_servers_never_queries_unspecified() {
        let mut t = FailingTransport { error: DnsError::Timeout };
        // Only 0.0.0.0 configured -> NoServer, not a query to 0.0.0.0.
        let result = resolve_with_servers(&mut t, "example.com", &[[0, 0, 0, 0]], 0);
        assert_eq!(result, Err(DnsError::NoServer));
    }

    #[test]
    fn resolve_with_servers_empty_is_no_server() {
        let mut t = FailingTransport { error: DnsError::Timeout };
        assert_eq!(
            resolve_with_servers(&mut t, "example.com", &[], 0),
            Err(DnsError::NoServer)
        );
    }

    #[test]
    fn resolve_with_servers_propagates_nxdomain() {
        struct EchoTransport(Vec<u8>);
        impl DnsTransport for EchoTransport {
            fn exchange(
                &mut self,
                _server: [u8; 4],
                query: &[u8],
                resp: &mut [u8],
            ) -> Result<usize, DnsError> {
                let id = u16::from_be_bytes([query[0], query[1]]);
                let mut r = self.0.clone();
                r[0..2].copy_from_slice(&id.to_be_bytes());
                let len = r.len().min(resp.len());
                resp[..len].copy_from_slice(&r[..len]);
                Ok(len)
            }
        }

        let mut t = EchoTransport(response_with_a("nope.example.com", &[], 0, 3, false));
        let servers = [[8, 8, 8, 8], [8, 8, 4, 4]];
        assert_eq!(
            resolve_with_servers(&mut t, "nope.example.com", &servers, 0),
            Err(DnsError::NxDomain)
        );
    }

    #[test]
    fn resolve_with_servers_reports_timeout() {
        let mut t = FailingTransport { error: DnsError::Timeout };
        assert_eq!(
            resolve_with_servers(&mut t, "example.com", &[[8, 8, 8, 8]], 0),
            Err(DnsError::Timeout)
        );
    }

    #[test]
    fn resolve_with_servers_rejects_invalid_hostname() {
        let mut t = FailingTransport { error: DnsError::Timeout };
        assert_eq!(
            resolve_with_servers(&mut t, "bad name", &[[8, 8, 8, 8]], 0),
            Err(DnsError::InvalidHostname)
        );
    }

    #[test]
    fn transport_error_is_propagated() {
        let mut t = FailingTransport { error: DnsError::Timeout };
        assert_eq!(
            query_server(&mut t, "example.com", [8, 8, 8, 8], 1),
            Err(DnsError::Timeout)
        );
    }

    #[test]
    fn parse_dotted_ip_variants() {
        assert_eq!(parse_dotted_ip("8.8.8.8"), Some([8, 8, 8, 8]));
        assert_eq!(parse_dotted_ip("192.168.1.1"), Some([192, 168, 1, 1]));
        assert_eq!(parse_dotted_ip("invalid"), None);
        assert_eq!(parse_dotted_ip("1.2.3"), None);
        assert_eq!(parse_dotted_ip("1.2.3.4.5"), None);
        assert_eq!(parse_dotted_ip("256.1.1.1"), None);
    }

    #[test]
    fn localhost_and_unspecified_helpers() {
        assert!(is_localhost("localhost"));
        assert!(is_localhost("LOCALHOST"));
        assert!(!is_localhost("localhost.example.com"));
        assert!(is_unspecified([0, 0, 0, 0]));
        assert!(!is_unspecified([0, 0, 0, 1]));
    }

    /// Optional integration test against real public DNS servers.
    ///
    /// Ignored by default so the unit suite never depends on the network.
    /// Run with: `cargo test -- --ignored`.
    #[test]
    #[ignore = "requires network access"]
    fn resolve_public_name_over_real_udp() {
        use std::net::UdpSocket;
        use std::time::Duration;

        struct StdTransport;

        impl DnsTransport for StdTransport {
            fn exchange(
                &mut self,
                server: [u8; 4],
                query: &[u8],
                resp: &mut [u8],
            ) -> Result<usize, DnsError> {
                let sock = UdpSocket::bind("0.0.0.0:0").map_err(|_| DnsError::Network)?;
                let _ = sock.set_read_timeout(Some(Duration::from_secs(3)));
                let addr = std::net::SocketAddrV4::new(
                    std::net::Ipv4Addr::new(server[0], server[1], server[2], server[3]),
                    DNS_PORT,
                );
                sock.send_to(query, addr)
                    .map_err(|_| DnsError::ServerUnreachable)?;
                let (n, _) = sock.recv_from(resp).map_err(|_| DnsError::Timeout)?;
                Ok(n)
            }
        }

        let mut transport = StdTransport;
        let servers = [[1, 1, 1, 1], [8, 8, 8, 8]];
        let (answer, server) =
            resolve_with_servers(&mut transport, "example.com", &servers, 1).unwrap();
        assert!(!answer.addresses.is_empty());
        assert!(servers.contains(&server));
    }
