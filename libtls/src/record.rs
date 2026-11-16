//! TLS 1.3 record layer (RFC 8446 §5) with ChaCha20-Poly1305.

use alloc::vec::Vec;

use libcrypto::{AeadError, ChaCha20Poly1305, TAG_LEN};

use crate::TlsError;

pub const CT_CHANGE_CIPHER_SPEC: u8 = 20;
pub const CT_ALERT: u8 = 21;
pub const CT_HANDSHAKE: u8 = 22;
pub const CT_APPLICATION_DATA: u8 = 23;

/// Maximum plaintext per record (RFC 8446 §5.1).
pub const MAX_PLAINTEXT: usize = 1 << 14;
/// Maximum accepted record payload (plaintext + tag + padding).
pub const MAX_RECORD: usize = MAX_PLAINTEXT + 256;

/// Traffic keys for one direction.
pub struct Keys {
    key: [u8; 32],
    iv: [u8; 12],
    seq: u64,
}

impl Keys {
    pub fn new(key: [u8; 32], iv: [u8; 12]) -> Self {
        Keys { key, iv, seq: 0 }
    }

    fn nonce(&self) -> [u8; 12] {
        let mut nonce = self.iv;
        let seq = self.seq.to_be_bytes();
        for i in 0..8 {
            nonce[4 + i] ^= seq[i];
        }
        nonce
    }

    /// Encrypt `data` and append a complete record to `out`.
    pub fn seal(&mut self, content_type: u8, data: &[u8], out: &mut Vec<u8>) -> Result<(), TlsError> {
        if data.len() > MAX_PLAINTEXT {
            return Err(TlsError::RecordLimit);
        }
        let inner_len = data.len() + 1;
        let header = record_header(CT_APPLICATION_DATA, inner_len + TAG_LEN);
        let mut plaintext = Vec::with_capacity(inner_len);
        plaintext.extend_from_slice(data);
        plaintext.push(content_type);
        let mut ciphertext = alloc::vec![0u8; inner_len];
        let tag = ChaCha20Poly1305::seal(&self.key, &self.nonce(), &header, &plaintext, &mut ciphertext)
            .map_err(map_aead)?;
        self.seq += 1;
        out.extend_from_slice(&header);
        out.extend_from_slice(&ciphertext);
        out.extend_from_slice(&tag);
        Ok(())
    }

    /// Decrypt `payload` (without the 5-byte header). Returns
    /// `(content_type, plaintext)`.
    pub fn open(
        &mut self,
        header: &[u8; 5],
        payload: &[u8],
    ) -> Result<(u8, Vec<u8>), TlsError> {
        if payload.len() < TAG_LEN + 1 {
            return Err(TlsError::RecordLimit);
        }
        let ciphertext = &payload[..payload.len() - TAG_LEN];
        let mut tag = [0u8; TAG_LEN];
        tag.copy_from_slice(&payload[payload.len() - TAG_LEN..]);
        let mut plaintext = alloc::vec![0u8; ciphertext.len()];
        ChaCha20Poly1305::open(&self.key, &self.nonce(), header, ciphertext, &tag, &mut plaintext)
            .map_err(map_aead)?;
        self.seq += 1;
        // Strip zero padding; the last non-zero byte is the real content type.
        let mut end = plaintext.len();
        while end > 0 && plaintext[end - 1] == 0 {
            end -= 1;
        }
        if end == 0 {
            return Err(TlsError::Protocol);
        }
        let content_type = plaintext[end - 1];
        plaintext.truncate(end - 1);
        Ok((content_type, plaintext))
    }
}

fn map_aead(e: AeadError) -> TlsError {
    match e {
        AeadError::AuthFailed => TlsError::BadRecord,
        AeadError::BufferTooSmall => TlsError::RecordLimit,
    }
}

/// 5-byte record header.
pub fn record_header(content_type: u8, len: usize) -> [u8; 5] {
    [
        content_type,
        0x03,
        0x03,
        ((len >> 8) & 0xff) as u8,
        (len & 0xff) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_seal_open_roundtrip_and_sequence() {
        let mut tx = Keys::new([7u8; 32], [9u8; 12]);
        let mut rx = Keys::new([7u8; 32], [9u8; 12]);
        for i in 0..3u8 {
            let mut out = Vec::new();
            tx.seal(CT_HANDSHAKE, &[i; 5], &mut out).unwrap();
            let header: [u8; 5] = out[..5].try_into().unwrap();
            assert_eq!(header[0], CT_APPLICATION_DATA);
            let (ct, plain) = rx.open(&header, &out[5..]).unwrap();
            assert_eq!(ct, CT_HANDSHAKE);
            assert_eq!(plain, &[i; 5]);
        }
    }

    #[test]
    fn record_rejects_tampering_and_limits() {
        let mut tx = Keys::new([1u8; 32], [2u8; 12]);
        let mut rx = Keys::new([1u8; 32], [2u8; 12]);
        let mut out = Vec::new();
        tx.seal(CT_APPLICATION_DATA, b"hello", &mut out).unwrap();
        let header: [u8; 5] = out[..5].try_into().unwrap();
        let mut bad = out[5..].to_vec();
        bad[0] ^= 1;
        assert_eq!(rx.open(&header, &bad).unwrap_err(), TlsError::BadRecord);
        // Oversized plaintext.
        let big = alloc::vec![0u8; MAX_PLAINTEXT + 1];
        let mut out = Vec::new();
        assert_eq!(
            tx.seal(CT_APPLICATION_DATA, &big, &mut out),
            Err(TlsError::RecordLimit)
        );
    }
}
