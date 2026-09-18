use aes::Aes128;
use cbc::Encryptor;
use cbc::cipher::{BlockModeEncrypt, KeyIvInit, block_padding::Pkcs7};
use riscbox::crypto::{CryptoError, decrypt_legacy_file, derive_key};

#[test]
fn pbkdf2_matches_the_sha256_known_answer() {
    let mut output = [0; 32];
    derive_key(b"password", b"salt", 2, &mut output).expect("valid derivation");
    assert_eq!(
        output,
        [
            0xae, 0x4d, 0x0c, 0x95, 0xaf, 0x6b, 0x46, 0xd3, 0x2d, 0x0a, 0xdf, 0xf9, 0x28, 0xf0,
            0x6d, 0xd0, 0x2a, 0x30, 0x3f, 0x8e, 0xf3, 0xc2, 0x51, 0xdf, 0xd6, 0xe2, 0xd8, 0x5a,
            0x95, 0x47, 0x4c, 0x43,
        ]
    );
    assert_eq!(
        derive_key(b"p", b"s", 0, &mut output),
        Err(CryptoError::InvalidIterations)
    );
}

#[test]
fn legacy_file_decrypts_and_rejects_malformed_inputs() {
    let key = [0x31; 16];
    let iv = [0x72; 16];
    let plaintext = b"browser filesystem payload";
    let mut encrypted = vec![0; plaintext.len() + 16];
    encrypted[..plaintext.len()].copy_from_slice(plaintext);
    let ciphertext = Encryptor::<Aes128>::new((&key).into(), (&iv).into())
        .encrypt_padded::<Pkcs7>(&mut encrypted, plaintext.len())
        .expect("sufficient padding storage")
        .to_vec();
    let mut file = vec![0xfb, 0xa2, 0xe9, 0x01];
    file.extend_from_slice(&iv);
    file.extend_from_slice(&ciphertext);
    assert_eq!(
        decrypt_legacy_file(&key, &mut file).expect("valid file"),
        plaintext
    );

    let mut bad_magic = file.clone();
    bad_magic[0] = 0;
    assert_eq!(
        decrypt_legacy_file(&key, &mut bad_magic),
        Err(CryptoError::InvalidHeader)
    );
    let mut truncated = vec![0xfb, 0xa2, 0xe9, 0x01];
    truncated.extend_from_slice(&iv);
    truncated.push(0);
    assert_eq!(
        decrypt_legacy_file(&key, &mut truncated),
        Err(CryptoError::InvalidCiphertext)
    );
}
