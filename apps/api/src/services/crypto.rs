use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, OsRng, generic_array::GenericArray},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use rand::{Rng, distributions::Alphanumeric};
use sha2::Sha256;
pub fn hash_key(value: &str, pepper: &[u8]) -> String {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(pepper).expect("valid HMAC key");
    mac.update(value.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}
pub fn generate_virtual_key() -> String {
    format!(
        "sk-gw_{}",
        rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(40)
            .map(char::from)
            .collect::<String>()
    )
}
pub fn encrypt(value: &str, key: &[u8; 32]) -> anyhow::Result<String> {
    let cipher = Aes256Gcm::new(GenericArray::from_slice(key));
    let mut nonce = [0u8; 12];
    rand::RngCore::fill_bytes(&mut OsRng, &mut nonce);
    let encrypted = cipher
        .encrypt(Nonce::from_slice(&nonce), value.as_bytes())
        .map_err(|_| anyhow::anyhow!("credential encryption failed"))?;
    Ok(STANDARD.encode([nonce.as_slice(), encrypted.as_slice()].concat()))
}
pub fn decrypt(value: &str, key: &[u8; 32]) -> anyhow::Result<String> {
    let bytes = STANDARD.decode(value)?;
    let (nonce, ciphertext) = bytes.split_at(12);
    Ok(String::from_utf8(
        Aes256Gcm::new(GenericArray::from_slice(key))
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| anyhow::anyhow!("credential decryption failed"))?,
    )?)
}
