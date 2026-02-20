use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use anyhow::{Context, Result, anyhow};
use base64::Engine;
use rand::Rng;

#[derive(Clone)]
pub struct EncryptionKey([u8; 32]);

impl EncryptionKey {
    pub fn from_base64(value: &str) -> Result<Self> {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(value.trim())
            .context("failed to base64-decode encryption key")?;
        let bytes: [u8; 32] = raw
            .try_into()
            .map_err(|_| anyhow!("encryption key must be 32 bytes after base64 decoding"))?;
        Ok(Self(bytes))
    }

    pub fn encrypt_str(&self, plaintext: &str) -> Result<EncryptedSecret> {
        let cipher = Aes256Gcm::new_from_slice(&self.0).expect("key length validated");

        let mut nonce = [0u8; 12];
        rand::rng().fill_bytes(&mut nonce);

        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
            .map_err(|_| anyhow!("encryption failed"))?;

        Ok(EncryptedSecret {
            ciphertext,
            nonce: nonce.to_vec(),
        })
    }

    pub fn decrypt_str(&self, ciphertext: &[u8], nonce: &[u8]) -> Result<String> {
        let cipher = Aes256Gcm::new_from_slice(&self.0).expect("key length validated");
        let nonce: [u8; 12] = nonce
            .try_into()
            .map_err(|_| anyhow!("nonce must be 12 bytes"))?;

        let plaintext = cipher
            .decrypt(Nonce::from_slice(&nonce), ciphertext)
            .map_err(|_| anyhow!("decryption failed"))?;
        let text = String::from_utf8(plaintext).context("token is not valid utf-8")?;
        Ok(text)
    }
}

#[derive(Debug, Clone)]
pub struct EncryptedSecret {
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
}
