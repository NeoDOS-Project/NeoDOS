#![allow(unused_imports)]
use crate::test_case;
use crate::test_eq;
use crate::test_true;
use alloc::format;
use alloc::vec;
use super::super::types::{TcpState, MacAddr, Ipv4Addr, SocketType, SocketDirection, SocketAddrV4};
use super::super::arp::ArpCache;
use super::super::socket::{
    SocketManager, SOCKET_MANAGER, socket_bind, socket_connect,
    socket_listen, socket_set_tcp_conn,
};
use super::super::tcp::{
    tcp_alloc_connection, tcp_bind, tcp_listen, tcp_connect, tcp_close,
    tcp_get_state, tcp_free_connection, tcp_send, tcp_recv, tcp_tick,
};
use super::super::nic::NicRegistry;
use super::super::ipv4::{compute_ip_checksum, build_ipv4_header, Ipv4Header};
use super::super::icmp::IcmpHeader;
use super::super::udp::UdpHeader;

use super::{CaptureNic, LinkNic, capture_nic};
pub fn register() {
    test_case!("dns_parse_a_response", {
        let ip = Ipv4Addr::new([8, 8, 8, 8]);
        let data = super::super::dns::test_make_a_response("google.com", ip, 42, 300);
        let response = super::super::dns::parse_dns_response(&data).unwrap();
        test_eq!(response.id, 42);
        test_eq!(response.answers.len(), 1);
        match &response.answers[0] {
            super::super::dns::DnsRecord::A { name, addr, ttl } => {
                test_eq!(name, "google.com");
                test_eq!(*addr, ip);
                test_eq!(*ttl, 300);
            }
            _ => panic!("Expected A record"),
        }
    });
    test_case!("dns_parse_cname_chain", {
        let ip = Ipv4Addr::new([142, 250, 80, 46]);
        let data = super::super::dns::test_make_cname_a_response("www.google.com", "forcesafesearch.google.com", ip, 7, 300);
        let response = super::super::dns::parse_dns_response(&data).unwrap();
        test_eq!(response.answers.len(), 2);

        match &response.answers[0] {
            super::super::dns::DnsRecord::Cname { name, cname, .. } => {
                test_eq!(name, "www.google.com");
                test_eq!(cname, "forcesafesearch.google.com");
            }
            _ => panic!("Expected CNAME record"),
        }

        let resolved = super::super::dns::resolve_cname_chain(&response);
        test_true!(resolved.is_some());
        test_eq!(resolved.unwrap(), ip);
    });
    test_case!("dns_cache_hit_ttl", {
        let mut cache = super::super::dns::DnsCache::new();
        let ip = Ipv4Addr::new([8, 8, 8, 8]);
        cache.insert("google.com", ip);

        let cached = cache.lookup("google.com");
        test_eq!(cached, Some(ip));

        cache.insert("example.com", Ipv4Addr::new([93, 184, 216, 34]));
        test_eq!(cache.len(), 2);
    });
    test_case!("dns_cache_expiry", {
        let mut cache = super::super::dns::DnsCache::new();
        cache.insert("short-ttl.example.com", Ipv4Addr::new([1, 2, 3, 4]));

        let ttl_ticks = super::super::dns::DNS_DEFAULT_TTL_SECS * 100 / super::super::dns::DNS_TICK_INTERVAL;
        for _ in 0..ttl_ticks + 20 {
            cache.tick();
        }

        let cached = cache.lookup("short-ttl.example.com");
        test_true!(cached.is_none());
    });
    test_case!("dns_resolve_localhost", {
        let ip = super::super::dns::dns_resolve("localhost");
        test_eq!(ip, Some(Ipv4Addr::localhost()));
    });
    test_case!("dns_encode_decode_name", {
        let encoded = super::super::dns::encode_dns_name("www.example.com");
        test_eq!(encoded, vec![3, b'w', b'w', b'w', 7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0]);

        let (decoded, _) = super::super::dns::decode_dns_name(&encoded, 0).unwrap();
        test_eq!(decoded, "www.example.com");
    });
    test_case!("dns_parse_dotted_ip", {
        let ip = super::super::dns::parse_dotted_ip("8.8.8.8");
        test_eq!(ip, Some(Ipv4Addr::new([8, 8, 8, 8])));

        let ip = super::super::dns::parse_dotted_ip("192.168.1.1");
        test_eq!(ip, Some(Ipv4Addr::new([192, 168, 1, 1])));

        let ip = super::super::dns::parse_dotted_ip("invalid");
        test_eq!(ip, None);
    });
    test_case!("dns_empty_response", {
        let data = super::super::dns::build_dns_query("nonexistent.example.com", super::super::dns::DNS_TYPE_A, 1);
        let result = super::super::dns::parse_dns_response(&data);
        test_true!(result.is_err());
    });
    test_case!("dns_build_query", {
        let query = super::super::dns::build_dns_query("example.com", super::super::dns::DNS_TYPE_A, 99);
        test_true!(query.len() > 12);

        let header: &super::super::dns::DnsHeader = unsafe { &*(query.as_ptr() as *const super::super::dns::DnsHeader) };
        test_eq!(header.id(), 99);
        test_true!(!header.is_response());
        test_eq!(header.qdcount(), 1);
    });
    test_case!("dns_cache_max_entries", {
        let mut cache = super::super::dns::DnsCache::new();
        for i in 0..70 {
            cache.insert(
                &alloc::format!("host{}.example.com", i),
                Ipv4Addr::new([10, 0, 0, i as u8]),
            );
        }
        test_true!(cache.len() <= super::super::dns::DNS_MAX_CACHE);
    });
    test_case!("dns_cache_clear", {
        let mut cache = super::super::dns::DnsCache::new();
        cache.insert("test.example.com", Ipv4Addr::new([1, 2, 3, 4]));
        cache.insert("test2.example.com", Ipv4Addr::new([5, 6, 7, 8]));
        test_eq!(cache.len(), 2);
        cache.clear();
        test_eq!(cache.len(), 0);
    });
}
