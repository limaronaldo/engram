//! Snapshot cryptography — AES-256-GCM encryption + Ed25519 signing

use crate::error::{EngramError, Result};

/// Encrypt data with AES-256-GCM.
///
/// Returns `nonce (12 bytes) || ciphertext` as a single byte vector.
pub fn encrypt_aes256(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };
    use rand::RngCore;

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| EngramError::Encryption(format!("Invalid key: {}", e)))?;

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|e| EngramError::Encryption(format!("Encryption failed: {}", e)))?;

    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// Decrypt AES-256-GCM data formatted as `nonce (12 bytes) || ciphertext`.
pub fn decrypt_aes256(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };

    if data.len() < 12 {
        return Err(EngramError::Encryption(
            "Data too short to contain nonce".to_string(),
        ));
    }

    let (nonce_bytes, ciphertext) = data.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| EngramError::Encryption(format!("Invalid key: {}", e)))?;
    let nonce = Nonce::from_slice(nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| EngramError::Encryption(format!("Decryption failed: {}", e)))
}

/// Sign `data` with Ed25519 using `secret_key_bytes` (32-byte seed).
///
/// Returns the 64-byte signature.
pub fn sign_ed25519(data: &[u8], secret_key_bytes: &[u8; 32]) -> Result<Vec<u8>> {
    use ed25519_dalek::{Signer, SigningKey};

    let signing_key = SigningKey::from_bytes(secret_key_bytes);
    let signature = signing_key.sign(data);
    Ok(signature.to_bytes().to_vec())
}

/// Verify an Ed25519 signature.
///
/// Returns `true` if the signature is valid, `false` if it is not.
pub fn verify_ed25519(
    data: &[u8],
    signature_bytes: &[u8],
    public_key_bytes: &[u8; 32],
) -> Result<bool> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let verifying_key = VerifyingKey::from_bytes(public_key_bytes)
        .map_err(|e| EngramError::Encryption(format!("Invalid public key: {}", e)))?;

    let signature = Signature::from_slice(signature_bytes)
        .map_err(|e| EngramError::Encryption(format!("Invalid signature: {}", e)))?;

    match verifying_key.verify(data, &signature) {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Derive the Ed25519 public key from a 32-byte secret key seed.
pub fn public_key_from_secret(secret_key_bytes: &[u8; 32]) -> [u8; 32] {
    use ed25519_dalek::SigningKey;
    let signing_key = SigningKey::from_bytes(secret_key_bytes);
    signing_key.verifying_key().to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = [42u8; 32];
        let plaintext = b"hello engram snapshot";

        let ciphertext = encrypt_aes256(plaintext, &key).expect("encrypt");
        assert!(ciphertext.len() > 12, "ciphertext should be longer than nonce");
        assert_ne!(&ciphertext[12..], plaintext);

        let decrypted = decrypt_aes256(&ciphertext, &key).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_decrypt_wrong_key_fails() {
        let key = [1u8; 32];
        let wrong_key = [2u8; 32];
        let ciphertext = encrypt_aes256(b"secret", &key).expect("encrypt");
        assert!(decrypt_aes256(&ciphertext, &wrong_key).is_err());
    }

    #[test]
    fn test_decrypt_too_short_fails() {
        let key = [0u8; 32];
        let short = [0u8; 5];
        assert!(decrypt_aes256(&short, &key).is_err());
    }

    #[test]
    fn test_sign_verify_roundtrip() {
        let secret = [7u8; 32];
        let public = public_key_from_secret(&secret);
        let data = b"test payload for signing";

        let sig = sign_ed25519(data, &secret).expect("sign");
        assert_eq!(sig.len(), 64);

        let valid = verify_ed25519(data, &sig, &public).expect("verify");
        assert!(valid);
    }

    #[test]
    fn test_verify_wrong_data_fails() {
        let secret = [7u8; 32];
        let public = public_key_from_secret(&secret);
        let sig = sign_ed25519(b"original", &secret).expect("sign");

        let valid = verify_ed25519(b"tampered", &sig, &public).expect("verify");
        assert!(!valid);
    }
}
