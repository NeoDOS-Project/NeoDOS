//! Minimal TLS 1.3 client for NeoDOS (#712 / M10).
//!
//! Client only, TLS 1.3 only, one cipher suite (`TLS_CHACHA20_POLY1305_SHA256`),
//! X25519 key exchange, and a compiled-in trust anchor list. Server
//! authentication is mandatory: the chain, validity window, key signatures and
//! hostname are always checked, and the wall clock must be trusted (M7). There
//! is no "insecure" mode.
//!
//! The transport is a trait so the same code runs over `net.nxl` sockets in the
//! guest and over `std::net` in the host tests.

#![cfg_attr(not(test), no_std)]

extern crate alloc;
#[cfg(feature = "debug-log")]
extern crate std;

#[cfg(feature = "debug-log")]
macro_rules! probe {
    ($($arg:tt)*) => { std::eprintln!($($arg)*) };
}
#[cfg(not(feature = "debug-log"))]
macro_rules! probe {
    ($($arg:tt)*) => {};
}

pub mod der;
pub mod handshake;
pub mod record;
pub mod x509;

use alloc::vec::Vec;

use libcrypto::{x25519, Sha256};

use crate::handshake::{
    derive_handshake_secret, derive_secret, extract_zero_salt, handshake_message,
    hkdf_expand_label, master_secret, parse_server_hello, traffic_keys, HandshakeReader,
    HS_CERTIFICATE, HS_CERTIFICATE_VERIFY, HS_ENCRYPTED_EXTENSIONS, HS_FINISHED, HS_SERVER_HELLO,
    SUITE_CHACHA20_POLY1305_SHA256,
};
use crate::record::{
    record_header, Keys, CT_ALERT, CT_APPLICATION_DATA, CT_HANDSHAKE, MAX_RECORD,
};
use crate::x509::{parse_certificate, verify_chain, Certificate};

/// TLS failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsError {
    /// The transport failed.
    Io,
    /// The peer violated the protocol.
    Protocol,
    /// The server did not negotiate TLS 1.3.
    UnsupportedVersion,
    /// No shared cipher suite.
    NoSharedCipher,
    /// The certificate signature algorithm is not supported.
    UnsupportedSignature,
    /// A record failed authentication.
    BadRecord,
    /// A record exceeded the size limit.
    RecordLimit,
    /// The certificate chain is not anchored in the trust store.
    NotTrusted,
    /// The certificate does not match the requested host.
    HostnameMismatch,
    /// A certificate is expired.
    Expired,
    /// A certificate is not yet valid.
    NotYetValid,
    /// The wall clock is not trusted (M7).
    ClockUnsynced,
    /// The kernel CSPRNG is not backed by hardware (M8).
    RngUnavailable,
    /// The peer sent an alert.
    Alert,
    /// The peer closed the connection.
    Closed,
}

/// Byte-stream transport used by the handshake and the record layer.
pub trait Transport {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, TlsError>;
    fn write_all(&mut self, data: &[u8]) -> Result<(), TlsError>;
    fn close(&mut self);
}

/// Random source. The production implementation must check `RandomCaps`.
pub trait Rng {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), TlsError>;
}

/// Client configuration for one connection.
pub struct ClientConfig<'a> {
    /// DER-encoded trust anchors.
    pub roots: &'a [Vec<u8>],
    /// Trusted wall clock (M7).
    pub clock: &'a dyn Clock,
    /// CSPRNG (M8).
    pub rng: &'a mut dyn Rng,
    /// SNI name; `None` for IP literals (RFC 6066 forbids IP SNI).
    pub server_name: Option<&'a str>,
    /// Identity to verify against the leaf SAN.
    pub host: x509::Host<'a>,
}

/// An established TLS connection.
pub struct TlsStream<T: Transport> {
    transport: T,
    read_keys: Keys,
    write_keys: Keys,
    read_buf: Vec<u8>,
    closed: bool,
}

const X25519_BASE: [u8; 32] = [
    9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

/// Perform the TLS 1.3 handshake over `transport`.
pub fn connect<T: Transport>(
    mut transport: T,
    cfg: ClientConfig<'_>,
) -> Result<TlsStream<T>, TlsError> {
    let now = cfg.clock.now_unix().ok_or(TlsError::ClockUnsynced)?;
    let mut client_private = [0u8; 32];
    let mut random = [0u8; 32];
    let mut session_id = [0u8; 32];
    cfg.rng.fill(&mut client_private)?;
    cfg.rng.fill(&mut random)?;
    cfg.rng.fill(&mut session_id)?;
    let client_public = x25519(&client_private, &X25519_BASE);

    let client_hello = handshake::build_client_hello(
        &random,
        &session_id,
        &client_public,
        cfg.server_name,
    );
    let mut transcript = Sha256::new();
    transcript.update(&client_hello);
    send_handshake(&mut transport, &client_hello)?;

    // ── ServerHello (plaintext) ──
    let (content_type, payload) = read_record(&mut transport)?;
    probe!("probe: first record ct={content_type} len={} data={:02x?}", payload.len(), &payload[..payload.len().min(8)]);
    if content_type != CT_HANDSHAKE {
        return Err(close_with(&mut transport, TlsError::Protocol));
    }
    let mut reader = HandshakeReader::new();
    reader.push(&payload);
    let (msg_type, body) = reader.next_message().ok_or(TlsError::Protocol)?;
    if msg_type != HS_SERVER_HELLO {
        return Err(close_with(&mut transport, TlsError::Protocol));
    }
    probe!("probe: server hello record ct={content_type} body={}", body.len());
    let server_hello = match parse_server_hello(&body) {
        Ok(sh) => sh,
        Err(e) => return Err(close_with(&mut transport, e)),
    };
    probe!("probe: suite={:#06x} keyshare={:?}", server_hello.cipher_suite, server_hello.server_key_share.is_some());
    if server_hello.cipher_suite != SUITE_CHACHA20_POLY1305_SHA256 {
        return Err(close_with(&mut transport, TlsError::NoSharedCipher));
    }
    let server_public = match server_hello.server_key_share {
        Some(k) => k,
        None => return Err(close_with(&mut transport, TlsError::NoSharedCipher)),
    };
    let mut sh_message = Vec::with_capacity(4 + body.len());
    sh_message.extend_from_slice(&handshake_message(HS_SERVER_HELLO, &body));
    transcript.update(&sh_message);

    // ── Handshake keys ──
    let shared = x25519(&client_private, &server_public);
    let early = extract_zero_salt(&[0u8; 32]);
    let handshake_secret = derive_handshake_secret(&early, &shared);
    let th = transcript.clone().finalize();
    let client_hs = derive_secret(&handshake_secret, b"c hs traffic", &th);
    let server_hs = derive_secret(&handshake_secret, b"s hs traffic", &th);
    let (ck, civ) = traffic_keys(&client_hs);
    let (sk, siv) = traffic_keys(&server_hs);
    let mut client_hs_keys = Keys::new(ck, civ);
    let mut server_hs_keys = Keys::new(sk, siv);

    // ── Encrypted handshake ──
    let mut encrypted = Vec::new();
    let mut cert_chain: Option<Vec<Vec<u8>>> = None;
    let mut cert_verify: Option<(u16, Vec<u8>)> = None;
    let mut server_finished = false;

    while !server_finished {
        let (content_type, payload) = read_record(&mut transport)?;
        if content_type == CT_ALERT {
            return Err(close_with(&mut transport, TlsError::Alert));
        }
        if content_type == record::CT_CHANGE_CIPHER_SPEC {
            continue;
        }
        if content_type != CT_APPLICATION_DATA {
            return Err(close_with(&mut transport, TlsError::Protocol));
        }
        let (inner_type, plaintext) = match server_hs_keys.open(&last_header(&payload), &payload) {
            Ok(v) => v,
            Err(e) => return Err(close_with(&mut transport, e)),
        };
        if inner_type != CT_HANDSHAKE {
            return Err(close_with(&mut transport, TlsError::Protocol));
        }
        reader.push(&plaintext);
        while let Some((msg_type, body)) = reader.next_message() {
            probe!("probe: handshake msg type={msg_type} len={}", body.len());
            match msg_type {
                HS_ENCRYPTED_EXTENSIONS => {
                    encrypted.extend_from_slice(&handshake_message(msg_type, &body));
                    transcript.update(&handshake_message(msg_type, &body));
                }
                HS_CERTIFICATE => {
                    cert_chain = Some(parse_certificate_message(&body)?);
                    encrypted.extend_from_slice(&handshake_message(msg_type, &body));
                    transcript.update(&handshake_message(msg_type, &body));
                }
                HS_CERTIFICATE_VERIFY => {
                    let scheme = u16::from_be_bytes([body[0], body[1]]);
                    let len = u16::from_be_bytes([body[2], body[3]]) as usize;
                    let sig = body[4..4 + len].to_vec();
                    cert_verify = Some((scheme, sig));
                    transcript.update(&handshake_message(msg_type, &body));
                }
                HS_FINISHED => {
                    // Verify against the transcript *before* the server's
                    // Finished, then add it.
                    let th = transcript.clone().finalize();
                    let mut finished_key = [0u8; 32];
                    hkdf_expand_label(&server_hs, b"finished", &[], &mut finished_key)?;
                    let expected = libcrypto::hmac_sha256(&finished_key, &th);
                    if body.len() != 32 || expected != body[..] {
                        return Err(close_with(&mut transport, TlsError::BadRecord));
                    }
                    transcript.update(&handshake_message(msg_type, &body));
                    server_finished = true;
                    break;
                }
                _ => return Err(close_with(&mut transport, TlsError::Protocol)),
            }
        }
    }

    // Authentication: chain, validity, hostname.
    let chain = cert_chain.ok_or(TlsError::NotTrusted)?;
    let certs: Vec<Certificate<'_>> = {
        let mut v = Vec::with_capacity(chain.len());
        for der in &chain {
            v.push(parse_certificate(der).map_err(|_| TlsError::NotTrusted)?);
        }
        v
    };
    if certs.is_empty() {
        return Err(close_with(&mut transport, TlsError::NotTrusted));
    }
    if let Err(e) = verify_chain(&certs, cfg.roots, cfg.host, now) {
        let mapped = match e {
            x509::X509Error::HostnameMismatch => TlsError::HostnameMismatch,
            x509::X509Error::Expired => TlsError::Expired,
            x509::X509Error::NotYetValid => TlsError::NotYetValid,
            _ => TlsError::NotTrusted,
        };
        return Err(close_with(&mut transport, mapped));
    }

    // CertificateVerify over the transcript through Certificate.
    let (scheme, signature) = match cert_verify {
        Some(v) => v,
        None => return Err(close_with(&mut transport, TlsError::NotTrusted)),
    };
    let mut cv_transcript = Sha256::new();
    cv_transcript.update(&client_hello);
    cv_transcript.update(&sh_message);
    cv_transcript.update(&encrypted[..encrypted.len().saturating_sub(0)]);
    let cv_th = cv_transcript.finalize();
    if let Err(e) = verify_certificate_verify(&certs[0], scheme, &signature, &cv_th) {
        return Err(close_with(&mut transport, e));
    }

    // ── Application keys from the transcript through server Finished ──
    let master = master_secret(&handshake_secret);
    let th_app = transcript.clone().finalize();
    let client_ap = derive_secret(&master, b"c ap traffic", &th_app);
    let server_ap = derive_secret(&master, b"s ap traffic", &th_app);
    let (cak, caiv) = traffic_keys(&client_ap);
    let (sak, saiv) = traffic_keys(&server_ap);
    let client_ap_keys = Keys::new(cak, caiv);
    let server_ap_keys = Keys::new(sak, saiv);

    // ── Client Finished ──
    let th_fin = transcript.clone().finalize();
    let mut client_finished_key = [0u8; 32];
    hkdf_expand_label(&client_hs, b"finished", &[], &mut client_finished_key)?;
    let verify_data = libcrypto::hmac_sha256(&client_finished_key, &th_fin);
    let finished = handshake_message(HS_FINISHED, &verify_data);
    transcript.update(&finished);
    let mut out = Vec::new();
    client_hs_keys.seal(CT_HANDSHAKE, &finished, &mut out)?;
    transport.write_all(&out)?;

    // Middlebox compatibility: a ChangeCipherSpec is allowed/ignored.
    let _ = client_hs_keys;

    Ok(TlsStream {
        transport,
        read_keys: server_ap_keys,
        write_keys: client_ap_keys,
        read_buf: Vec::new(),
        closed: false,
    })
}

fn send_handshake<T: Transport>(transport: &mut T, message: &[u8]) -> Result<(), TlsError> {
    let header = record_header(CT_HANDSHAKE, message.len());
    transport.write_all(&header)?;
    transport.write_all(message)
}

fn read_record<T: Transport>(transport: &mut T) -> Result<(u8, Vec<u8>), TlsError> {
    let mut header = [0u8; 5];
    read_exact(transport, &mut header)?;
    let len = u16::from_be_bytes([header[3], header[4]]) as usize;
    if len == 0 || len > MAX_RECORD {
        return Err(TlsError::RecordLimit);
    }
    let mut payload = alloc::vec![0u8; len];
    read_exact(transport, &mut payload)?;
    Ok((header[0], payload))
}

fn read_exact<T: Transport>(transport: &mut T, buf: &mut [u8]) -> Result<(), TlsError> {
    let mut off = 0usize;
    while off < buf.len() {
        match transport.read(&mut buf[off..])? {
            0 => return Err(TlsError::Closed),
            n => off += n,
        }
    }
    Ok(())
}

/// The record header of the record just read; `open` needs it as AAD. The
/// caller passes the payload, so rebuild the header from its length.
fn last_header(payload: &[u8]) -> [u8; 5] {
    record_header(CT_APPLICATION_DATA, payload.len())
}

fn parse_certificate_message(body: &[u8]) -> Result<Vec<Vec<u8>>, TlsError> {
    // context (u8 len, must be 0 for the server), certificate_list (u24 len),
    // entries: cert_data (u24 len) + extensions (u16 len).
    if body.len() < 4 || body[0] != 0 {
        return Err(TlsError::Protocol);
    }
    let list_len = ((body[1] as usize) << 16) | ((body[2] as usize) << 8) | body[3] as usize;
    if body.len() < 4 + list_len {
        return Err(TlsError::Protocol);
    }
    let mut pos = 4usize;
    let end = 4 + list_len;
    let mut chain = Vec::new();
    while pos + 3 <= end {
        let len = ((body[pos] as usize) << 16)
            | ((body[pos + 1] as usize) << 8)
            | body[pos + 2] as usize;
        pos += 3;
        if pos + len > end {
            return Err(TlsError::Protocol);
        }
        chain.push(body[pos..pos + len].to_vec());
        pos += len;
        if pos + 2 <= end {
            let ext_len = u16::from_be_bytes([body[pos], body[pos + 1]]) as usize;
            pos += 2 + ext_len;
        }
    }
    Ok(chain)
}

fn verify_certificate_verify(
    leaf: &Certificate<'_>,
    scheme: u16,
    signature: &[u8],
    transcript_hash: &[u8],
) -> Result<(), TlsError> {
    let mut content = Vec::with_capacity(64 + 34 + 1 + 32);
    content.extend_from_slice(&[0x20u8; 64]);
    content.extend_from_slice(b"TLS 1.3, server CertificateVerify");
    content.push(0);
    content.extend_from_slice(transcript_hash);

    // The leaf SPKI is re-parsed by the x509 module.
    match scheme {
        handshake::SIG_RSA_PSS_RSAE_SHA256 => {
            let key = x509::rsa_public_key(leaf.spki).map_err(|_| TlsError::UnsupportedSignature)?;
            key.verify_pss_sha256(&content, signature, 32)
                .map_err(|_| TlsError::NotTrusted)
        }
        handshake::SIG_RSA_PKCS1_SHA256 => {
            let key = x509::rsa_public_key(leaf.spki).map_err(|_| TlsError::UnsupportedSignature)?;
            key.verify_pkcs1_sha256(&content, signature)
                .map_err(|_| TlsError::NotTrusted)
        }
        handshake::SIG_ECDSA_SECP256R1_SHA256 => {
            let (x, y) = x509::p256_public_key(leaf.spki).map_err(|_| TlsError::UnsupportedSignature)?;
            let mut der = der::Der::new(signature);
            let mut seq = der.expect_sequence().map_err(|_| TlsError::BadRecord)?;
            let r = der::integer_bytes(&seq.next_tlv().map_err(|_| TlsError::BadRecord)?)
                .map_err(|_| TlsError::BadRecord)?;
            let s = der::integer_bytes(&seq.next_tlv().map_err(|_| TlsError::BadRecord)?)
                .map_err(|_| TlsError::BadRecord)?;
            libcrypto::verify_p256_sha256(&x, &y, &content, r, s)
                .map_err(|_| TlsError::NotTrusted)
        }
        _ => Err(TlsError::UnsupportedSignature),
    }
}

fn close_with<T: Transport>(transport: &mut T, err: TlsError) -> TlsError {
    transport.close();
    err
}

impl<T: Transport> TlsStream<T> {
    /// Read application data (decrypting records as needed).
    pub fn read(&mut self, out: &mut [u8]) -> Result<usize, TlsError> {
        if out.is_empty() {
            return Ok(0);
        }
        loop {
            if !self.read_buf.is_empty() {
                let n = self.read_buf.len().min(out.len());
                out[..n].copy_from_slice(&self.read_buf[..n]);
                self.read_buf.drain(..n);
                return Ok(n);
            }
            if self.closed {
                return Ok(0);
            }
            let (content_type, payload) = read_record(&mut self.transport)?;
            match content_type {
                CT_ALERT => {
                    self.closed = true;
                    self.transport.close();
                    return Ok(0);
                }
                CT_APPLICATION_DATA => {}
                record::CT_CHANGE_CIPHER_SPEC => continue,
                _ => return Err(TlsError::Protocol),
            }
            let (inner_type, plaintext) =
                self.read_keys.open(&last_header(&payload), &payload)?;
            match inner_type {
                CT_ALERT => {
                    self.closed = true;
                    self.transport.close();
                    return Ok(0);
                }
                // Post-handshake messages (NewSessionTicket, KeyUpdate) are
                // ignored: resumption is not implemented.
                CT_HANDSHAKE => continue,
                CT_APPLICATION_DATA => self.read_buf = plaintext,
                _ => return Err(TlsError::Protocol),
            }
        }
    }

    /// Encrypt and send application data.
    pub fn write_all(&mut self, data: &[u8]) -> Result<(), TlsError> {
        let mut out = Vec::with_capacity(data.len() + 64);
        self.write_keys.seal(CT_APPLICATION_DATA, data, &mut out)?;
        self.transport.write_all(&out)
    }

    /// Send `close_notify` and close the transport.
    pub fn close(mut self) {
        let mut out = Vec::new();
        if self.write_keys.seal(CT_ALERT, &[0x01, 0x00], &mut out).is_ok() {
            let _ = self.transport.write_all(&out);
        }
        self.transport.close();
    }
}

/// Re-exported for tests and callers that need the signature scheme names.
pub use handshake::{
    SIG_ECDSA_SECP256R1_SHA256, SIG_RSA_PKCS1_SHA256, SIG_RSA_PSS_RSAE_SHA256,
};
/// Re-exported clock trait (the design's `Clock`).
pub use libclock::Clock;
pub use x509::{Host, X509Error};
