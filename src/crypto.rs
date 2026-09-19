//! Compatibility helpers for the legacy encrypted HTTP filesystem format.

use aes::Aes128;
use cbc::Decryptor;
use cbc::cipher::{BlockModeDecrypt, KeyIvInit, block_padding::Pkcs7};
use pbkdf2::{hmac::Hmac, pbkdf2};
use sha2::Sha256;

const MAGIC: [u8; 4] = [0xfb, 0xa2, 0xe9, 0x01];
const HEADER_LEN: usize = MAGIC.len() + 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CryptoError {
    InvalidHeader,
    InvalidCiphertext,
    InvalidIterations,
}

impl core::fmt::Display for CryptoError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "encrypted file error: {self:?}")
    }
}

impl std::error::Error for CryptoError {}

/// Derives bytes using the legacy PBKDF2-HMAC-SHA256 command semantics.
///
/// # Errors
///
/// Returns an error when `iterations` is zero or the derivation fails.
pub fn derive_key(
    password: &[u8],
    salt: &[u8],
    iterations: u32,
    output: &mut [u8],
) -> Result<(), CryptoError> {
    if iterations == 0 {
        return Err(CryptoError::InvalidIterations);
    }
    pbkdf2::<Hmac<Sha256>>(password, salt, iterations, output)
        .map_err(|_| CryptoError::InvalidIterations)
}

/// Decrypts `magic || IV || AES-128-CBC/PKCS#7 ciphertext` in place.
///
/// The returned slice aliases `file` and contains only the plaintext.
///
/// # Errors
///
/// Returns an error for a bad header, non-block-aligned ciphertext, or invalid
/// PKCS#7 padding.
pub fn decrypt_legacy_file<'a>(
    key: &[u8; 16],
    file: &'a mut [u8],
) -> Result<&'a [u8], CryptoError> {
    if file.len() < HEADER_LEN || file[..MAGIC.len()] != MAGIC {
        return Err(CryptoError::InvalidHeader);
    }
    let (header, ciphertext) = file.split_at_mut(HEADER_LEN);
    if ciphertext.is_empty() || ciphertext.len() % 16 != 0 {
        return Err(CryptoError::InvalidCiphertext);
    }
    let mut iv = [0; 16];
    iv.copy_from_slice(&header[MAGIC.len()..]);
    Decryptor::<Aes128>::new(key.into(), (&iv).into())
        .decrypt_padded::<Pkcs7>(ciphertext)
        .map_err(|_| CryptoError::InvalidCiphertext)
}
