//! TLS 1.3 handshake messages and the SHA-256 key schedule (RFC 8446 §7).

use alloc::vec::Vec;

use crate::TlsError;

pub const HS_CLIENT_HELLO: u8 = 1;
pub const HS_SERVER_HELLO: u8 = 2;
pub const HS_ENCRYPTED_EXTENSIONS: u8 = 8;
pub const HS_CERTIFICATE: u8 = 11;
pub const HS_CERTIFICATE_VERIFY: u8 = 15;
pub const HS_FINISHED: u8 = 20;

pub const GROUP_X25519: u16 = 0x001d;
pub const SUITE_CHACHA20_POLY1305_SHA256: u16 = 0x1303;
pub const TLS13_VERSION: u16 = 0x0304;

pub const EXT_SERVER_NAME: u16 = 0x0000;
pub const EXT_SUPPORTED_GROUPS: u16 = 0x000a;
pub const EXT_SIGNATURE_ALGORITHMS: u16 = 0x000d;
pub const EXT_ALPN: u16 = 0x0010;
pub const EXT_SUPPORTED_VERSIONS: u16 = 0x002b;
pub const EXT_KEY_SHARE: u16 = 0x0033;

pub const SIG_RSA_PSS_RSAE_SHA256: u16 = 0x0804;
pub const SIG_ECDSA_SECP256R1_SHA256: u16 = 0x0403;
pub const SIG_RSA_PKCS1_SHA256: u16 = 0x0401;

/// Reassembles handshake messages that may be split across records.
pub struct HandshakeReader {
    buf: Vec<u8>,
}

impl Default for HandshakeReader {
    fn default() -> Self {
        Self::new()
    }
}

impl HandshakeReader {
    pub fn new() -> Self {
        HandshakeReader { buf: Vec::new() }
    }

    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Next complete message, or `None` when more bytes are needed.
    pub fn next_message(&mut self) -> Option<(u8, Vec<u8>)> {
        if self.buf.len() < 4 {
            return None;
        }
        let len = ((self.buf[1] as usize) << 16) | ((self.buf[2] as usize) << 8) | self.buf[3] as usize;
        if self.buf.len() < 4 + len {
            return None;
        }
        let msg_type = self.buf[0];
        let body = self.buf[4..4 + len].to_vec();
        self.buf.drain(..4 + len);
        Some((msg_type, body))
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

/// Prefix a handshake message with its 4-byte header.
pub fn handshake_message(msg_type: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + body.len());
    out.push(msg_type);
    out.push(((body.len() >> 16) & 0xff) as u8);
    out.push(((body.len() >> 8) & 0xff) as u8);
    out.push((body.len() & 0xff) as u8);
    out.extend_from_slice(body);
    out
}

pub fn build_client_hello(
    random: &[u8; 32],
    session_id: &[u8; 32],
    key_share: &[u8; 32],
    server_name: Option<&str>,
) -> Vec<u8> {
    let mut body = Vec::with_capacity(200);
    body.extend_from_slice(&[0x03, 0x03]); // legacy_version
    body.extend_from_slice(random);
    body.push(session_id.len() as u8);
    body.extend_from_slice(session_id);
    body.extend_from_slice(&2u16.to_be_bytes()); // cipher_suites length
    body.extend_from_slice(&SUITE_CHACHA20_POLY1305_SHA256.to_be_bytes());
    body.push(1);
    body.push(0); // null compression

    let mut ext = Vec::new();
    if let Some(name) = server_name {
        let mut sn = Vec::new();
        sn.extend_from_slice(&((name.len() + 3) as u16).to_be_bytes());
        sn.push(0); // host_name
        sn.extend_from_slice(&(name.len() as u16).to_be_bytes());
        sn.extend_from_slice(name.as_bytes());
        push_ext(&mut ext, EXT_SERVER_NAME, &sn);
    }
    let mut groups = Vec::new();
    groups.extend_from_slice(&2u16.to_be_bytes());
    groups.extend_from_slice(&GROUP_X25519.to_be_bytes());
    push_ext(&mut ext, EXT_SUPPORTED_GROUPS, &groups);

    let mut sigs = Vec::new();
    sigs.extend_from_slice(&6u16.to_be_bytes());
    for sig in [
        SIG_RSA_PSS_RSAE_SHA256,
        SIG_ECDSA_SECP256R1_SHA256,
        SIG_RSA_PKCS1_SHA256,
    ] {
        sigs.extend_from_slice(&sig.to_be_bytes());
    }
    push_ext(&mut ext, EXT_SIGNATURE_ALGORITHMS, &sigs);

    let mut alpn = Vec::new();
    // ProtocolNameList: u16 length, then (u8 length + name).
    alpn.extend_from_slice(&9u16.to_be_bytes());
    alpn.push(8);
    alpn.extend_from_slice(b"http/1.1");
    push_ext(&mut ext, EXT_ALPN, &alpn);

    // ClientHello form: a 1-byte length followed by the offered versions.
    let versions = [2u8, 0x03, 0x04];
    push_ext(&mut ext, EXT_SUPPORTED_VERSIONS, &versions);

    let mut ks = Vec::new();
    ks.extend_from_slice(&36u16.to_be_bytes());
    ks.extend_from_slice(&GROUP_X25519.to_be_bytes());
    ks.extend_from_slice(&32u16.to_be_bytes());
    ks.extend_from_slice(key_share);
    push_ext(&mut ext, EXT_KEY_SHARE, &ks);

    body.extend_from_slice(&(ext.len() as u16).to_be_bytes());
    body.extend_from_slice(&ext);
    handshake_message(HS_CLIENT_HELLO, &body)
}

fn push_ext(out: &mut Vec<u8>, ext_type: u16, data: &[u8]) {
    out.extend_from_slice(&ext_type.to_be_bytes());
    out.extend_from_slice(&(data.len() as u16).to_be_bytes());
    out.extend_from_slice(data);
}

fn read_u16(data: &[u8], pos: &mut usize) -> Result<u16, TlsError> {
    if *pos + 2 > data.len() {
        return Err(TlsError::Protocol);
    }
    let v = u16::from_be_bytes([data[*pos], data[*pos + 1]]);
    *pos += 2;
    Ok(v)
}

/// Parsed ServerHello.
pub struct ServerHello {
    pub random: [u8; 32],
    pub cipher_suite: u16,
    pub server_key_share: Option<[u8; 32]>,
}

pub fn parse_server_hello(body: &[u8]) -> Result<ServerHello, TlsError> {
    let mut pos = 0usize;
    let version = read_u16(body, &mut pos)?;
    if version != 0x0303 {
        return Err(TlsError::Protocol);
    }
    if pos + 32 > body.len() {
        return Err(TlsError::Protocol);
    }
    let mut random = [0u8; 32];
    random.copy_from_slice(&body[pos..pos + 32]);
    pos += 32;
    let sid_len = *body.get(pos).ok_or(TlsError::Protocol)? as usize;
    pos += 1 + sid_len;
    let cipher_suite = read_u16(body, &mut pos)?;
    let compression = *body.get(pos).ok_or(TlsError::Protocol)?;
    pos += 1;
    if compression != 0 {
        return Err(TlsError::Protocol);
    }
    let ext_len = read_u16(body, &mut pos)? as usize;
    if pos + ext_len > body.len() {
        return Err(TlsError::Protocol);
    }
    let ext = &body[pos..pos + ext_len];

    let mut has_tls13 = false;
    let mut server_key_share = None;
    let mut i = 0usize;
    while i + 4 <= ext.len() {
        let ext_type = u16::from_be_bytes([ext[i], ext[i + 1]]);
        let len = u16::from_be_bytes([ext[i + 2], ext[i + 3]]) as usize;
        i += 4;
        if i + len > ext.len() {
            return Err(TlsError::Protocol);
        }
        let data = &ext[i..i + len];
        match ext_type {
            EXT_SUPPORTED_VERSIONS => {
                if data.len() != 2 || u16::from_be_bytes([data[0], data[1]]) != TLS13_VERSION {
                    return Err(TlsError::Protocol);
                }
                has_tls13 = true;
            }
            EXT_KEY_SHARE => {
                if data.len() >= 4 {
                    let group = u16::from_be_bytes([data[0], data[1]]);
                    let klen = u16::from_be_bytes([data[2], data[3]]) as usize;
                    if group == GROUP_X25519 && klen == 32 && data.len() >= 4 + 32 {
                        let mut key = [0u8; 32];
                        key.copy_from_slice(&data[4..36]);
                        server_key_share = Some(key);
                    }
                }
            }
            _ => {}
        }
        i += len;
    }
    if !has_tls13 {
        return Err(TlsError::Protocol);
    }
    Ok(ServerHello {
        random,
        cipher_suite,
        server_key_share,
    })
}

// ── Key schedule ──

pub fn hkdf_expand_label(
    secret: &[u8],
    label: &[u8],
    context: &[u8],
    out: &mut [u8],
) -> Result<(), TlsError> {
    let mut full = Vec::with_capacity(6 + label.len());
    full.extend_from_slice(b"tls13 ");
    full.extend_from_slice(label);
    let mut info = Vec::with_capacity(4 + full.len() + context.len());
    info.extend_from_slice(&(out.len() as u16).to_be_bytes());
    info.push(full.len() as u8);
    info.extend_from_slice(&full);
    info.push(context.len() as u8);
    info.extend_from_slice(context);
    libcrypto::hkdf_expand(secret, &info, out).map_err(|_| TlsError::Protocol)
}

pub fn derive_secret(secret: &[u8], label: &[u8], transcript_hash: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    hkdf_expand_label(secret, label, transcript_hash, &mut out).expect("HKDF expand");
    out
}

/// HKDF-Extract with an all-zero salt (empty PSK).
pub fn extract_zero_salt(ikm: &[u8]) -> [u8; 32] {
    let zero = [0u8; 32];
    libcrypto::hkdf_extract(&zero, ikm)
}

/// `Derive-Secret(secret, "derived", "")`.
pub fn derive_handshake_secret(early: &[u8], ecdhe: &[u8]) -> [u8; 32] {
    let empty_hash = libcrypto::Sha256::digest(&[]);
    let derived = derive_secret(early, b"derived", &empty_hash);
    libcrypto::hkdf_extract(&derived, ecdhe)
}

pub fn master_secret(handshake: &[u8]) -> [u8; 32] {
    let empty_hash = libcrypto::Sha256::digest(&[]);
    let derived = derive_secret(handshake, b"derived", &empty_hash);
    libcrypto::hkdf_extract(&derived, &[0u8; 32])
}

pub fn traffic_keys(secret: &[u8]) -> ([u8; 32], [u8; 12]) {
    let mut key = [0u8; 32];
    let mut iv = [0u8; 12];
    hkdf_expand_label(secret, b"key", &[], &mut key).expect("key");
    hkdf_expand_label(secret, b"iv", &[], &mut iv).expect("iv");
    (key, iv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_reader_reassembles_fragments() {
        let msg = handshake_message(HS_FINISHED, &[1, 2, 3]);
        let mut r = HandshakeReader::new();
        r.push(&msg[..2]);
        assert!(r.next_message().is_none());
        r.push(&msg[2..]);
        let (ty, body) = r.next_message().unwrap();
        assert_eq!(ty, HS_FINISHED);
        assert_eq!(body, &[1, 2, 3]);
        assert!(r.next_message().is_none());
    }

    #[test]
    fn client_hello_has_the_required_extensions() {
        let random = [1u8; 32];
        let sid = [2u8; 32];
        let key = [3u8; 32];
        let ch = build_client_hello(&random, &sid, &key, Some("example.com"));
        assert_eq!(ch[0], HS_CLIENT_HELLO);
        let body = &ch[4..];
        assert_eq!(&body[..2], &[0x03, 0x03]);
        assert_eq!(&body[2..34], &random);
        let mut pos = 34;
        let sid_len = body[pos] as usize;
        pos += 1 + sid_len;
        let suite_len = u16::from_be_bytes([body[pos], body[pos + 1]]) as usize;
        pos += 2 + suite_len;
        let comp_len = body[pos] as usize;
        pos += 1 + comp_len;
        let ext_len = u16::from_be_bytes([body[pos], body[pos + 1]]) as usize;
        pos += 2;
        let ext = &body[pos..pos + ext_len];
        let mut types = Vec::new();
        let mut i = 0;
        while i + 4 <= ext.len() {
            let ty = u16::from_be_bytes([ext[i], ext[i + 1]]);
            let len = u16::from_be_bytes([ext[i + 2], ext[i + 3]]) as usize;
            types.push(ty);
            i += 4 + len;
        }
        for expected in [
            EXT_SERVER_NAME,
            EXT_SUPPORTED_GROUPS,
            EXT_SIGNATURE_ALGORITHMS,
            EXT_ALPN,
            EXT_SUPPORTED_VERSIONS,
            EXT_KEY_SHARE,
        ] {
            assert!(types.contains(&expected), "missing extension {expected:#06x}");
        }
    }

    #[test]
    fn key_schedule_is_deterministic_and_label_bound() {
        let early = extract_zero_salt(&[]);
        let hs = derive_handshake_secret(&early, &[9u8; 32]);
        let (key, iv) = traffic_keys(&hs);
        let (key2, iv2) = traffic_keys(&hs);
        assert_eq!(key, key2);
        assert_eq!(iv, iv2);
        // A different secret yields different keys.
        let hs2 = derive_handshake_secret(&early, &[8u8; 32]);
        assert_ne!(traffic_keys(&hs2).0, key);
        // Master secret differs from the handshake secret.
        let ms = master_secret(&hs);
        assert_ne!(ms, hs);
    }
}
