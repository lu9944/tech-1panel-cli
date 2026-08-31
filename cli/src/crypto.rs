use aes::Aes256;
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use cbc::Encryptor;
use cipher::generic_array::GenericArray;
use cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use rand::rngs::OsRng;
use rand::RngCore;
use rsa::pkcs8::DecodePublicKey;
use rsa::{Pkcs1v15Encrypt, RsaPublicKey};

type Aes256CbcEnc = Encryptor<Aes256>;

/// Encrypt the login password the same way the 1Panel web frontend does:
/// generate a 16-byte AES key, RSA(PKCS1v15) encrypt the hex string of that key,
/// then AES-256-CBC (PKCS7) encrypt the password, return `keyCipher:iv:cipher`.
pub fn encrypt_password(password: &str, public_key_pem: &str) -> Result<String> {
    let mut aes_raw = [0u8; 16];
    OsRng.fill_bytes(&mut aes_raw);
    let aes_key_hex = hex::encode(aes_raw);
    let aes_key_bytes = aes_key_hex.as_bytes();

    let pubkey = RsaPublicKey::from_public_key_pem(public_key_pem)
        .map_err(|e| anyhow!("解析面板 RSA 公钥失败: {e}"))?;
    let key_cipher = pubkey
        .encrypt(&mut OsRng, Pkcs1v15Encrypt, aes_key_bytes)
        .map_err(|e| anyhow!("RSA 加密 AES 密钥失败: {e}"))?;

    let mut iv = [0u8; 16];
    OsRng.fill_bytes(&mut iv);

    let key = GenericArray::from_slice(aes_key_bytes);
    let iv_arr = GenericArray::from_slice(&iv);
    let enc = Aes256CbcEnc::new(key, iv_arr);
    let padded_len = password.len() + 16 - (password.len() % 16);
    let mut buf = vec![0u8; padded_len];
    buf[..password.len()].copy_from_slice(password.as_bytes());
    let ciphertext = enc
        .encrypt_padded_mut::<Pkcs7>(&mut buf, password.len())
        .map_err(|e| anyhow!("AES 加密失败: {e:?}"))?;

    Ok(format!(
        "{}:{}:{}",
        BASE64.encode(key_cipher),
        BASE64.encode(iv),
        BASE64.encode(ciphertext)
    ))
}

/// Optional debug helpers (behind PANEL_CLI_DEBUG env var).
pub fn debug_public_key(public_key_pem: &str) {
    if std::env::var("PANEL_CLI_DEBUG").is_ok() {
        eprintln!("[debug] public key PEM:\n{public_key_pem}");
    }
}

/// URL-decode a percent-encoded string while preserving literal `+`.
pub fn url_decode(s: &str) -> Result<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = hex_val(bytes[i + 1])?;
            let lo = hex_val(bytes[i + 2])?;
            out.push(hi * 16 + lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Ok(String::from_utf8(out)?)
}

fn hex_val(b: u8) -> Result<u8> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(anyhow!("URL 解码失败: 非法的转义序列 %{}", b as char)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_decode_keeps_plus() {
        assert_eq!(url_decode("a+b%20c%3D").unwrap(), "a+b c=");
    }
}
