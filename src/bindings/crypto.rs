//! Cryptographic functions binding module.
//!
//! This module provides cryptographic utilities for hashing, encoding, and encryption.
//! Import via `require("@lmb/crypto")`.
//!
//! # Available Methods
//!
//! ## Encoding
//! - `base64_encode(data)` - Encode data to base64 string.
//! - `base64_decode(data)` - Decode base64 string to data.
//!
//! ## Hashing
//! - `crc32(data)` - Compute CRC32 checksum (hex string).
//! - `md5(data)` - Compute MD5 hash (hex string).
//! - `sha1(data)` - Compute SHA-1 hash (hex string).
//! - `sha256(data)` - Compute SHA-256 hash (hex string).
//! - `sha384(data)` - Compute SHA-384 hash (hex string).
//! - `sha512(data)` - Compute SHA-512 hash (hex string).
//! - `hmac(algorithm, data, secret)` - Compute HMAC (sha1, sha256, sha384, sha512).
//!
//! ## Encryption/Decryption
//! - `encrypt(cipher, data, key, iv)` - Encrypt data using specified cipher.
//! - `decrypt(cipher, encrypted, key, iv)` - Decrypt data using specified cipher.
//!
//! # Supported Ciphers
//!
//! | Cipher     | Key Size | IV Size | Notes                    |
//! |------------|----------|---------|--------------------------|
//! | `aes-cbc`  | 16 bytes | 16 bytes| AES-128 in CBC mode      |
//! | `des-cbc`  | 8 bytes  | 8 bytes | DES in CBC mode          |
//! | `des-ecb`  | 8 bytes  | N/A     | DES in ECB mode (no IV)  |
//!
//! # Security Warning
//!
//! - **DES is considered insecure** for modern applications. Use AES when possible.
//! - **MD5 and SHA-1** should not be used for security-critical hashing.
//! - Always use cryptographically secure random values for keys and IVs.
//!
//! # Example
//!
//! ```lua
//! local crypto = require("@lmb/crypto")
//!
//! -- Base64 encoding
//! local encoded = crypto.base64_encode("Hello")
//! local decoded = crypto.base64_decode(encoded)
//!
//! -- Hashing
//! local hash = crypto.sha256("password")
//!
//! -- HMAC
//! local hmac = crypto.hmac("sha256", "message", "secret")
//!
//! -- AES encryption (key and IV must be 16 bytes)
//! local key = "1234567890123456"
//! local iv = "abcdefghijklmnop"
//! local encrypted = crypto.encrypt("aes-cbc", "plaintext", key, iv)
//! local decrypted = crypto.decrypt("aes-cbc", encrypted, key, iv)
//! ```

use std::{fmt, sync::Arc};

use aes::cipher::{BlockDecryptMut as _, BlockEncryptMut as _, block_padding::Pkcs7};
use base64::prelude::*;
use crypto_common::{KeyInit, KeyIvInit as _};
use hmac::{Hmac, Mac};
use md5::Md5;
use mlua::prelude::*;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384, Sha512};

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
type DesCbcEnc = cbc::Encryptor<des::Des>;
type DesCbcDec = cbc::Decryptor<des::Des>;
type DesEcbEnc = ecb::Encryptor<des::Des>;
type DesEcbDec = ecb::Decryptor<des::Des>;

fn bad_argument(func: &str, pos: usize, message: impl fmt::Display) -> LuaError {
    LuaError::BadArgument {
        to: Some(func.to_string()),
        pos,
        name: None,
        cause: Arc::new(LuaError::external(message.to_string())),
    }
}

fn hash<H: Digest>(payload: &str) -> String {
    base16ct::lower::encode_string(&H::digest(payload))
}

fn hmac_hash<T: Mac + KeyInit>(secret: &str, payload: &str) -> mlua::Result<String> {
    let mut hasher = <T as KeyInit>::new_from_slice(secret.as_bytes()).into_lua_err()?;
    hasher.update(payload.as_bytes());
    let hash = hasher.finalize().into_bytes();
    Ok(base16ct::lower::encode_string(&hash))
}

pub(crate) struct CryptoBinding;

impl LuaUserData for CryptoBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_function("base64_encode", |_, data: String| {
            Ok(BASE64_STANDARD.encode(data.as_bytes()))
        });
        methods.add_function("base64_decode", |_, data: String| {
            let decoded = BASE64_STANDARD.decode(data.as_bytes()).into_lua_err()?;
            String::from_utf8(decoded).into_lua_err()
        });
        methods.add_function("crc32", |_, data: String| {
            Ok(format!("{:x}", crc32fast::hash(data.as_bytes())))
        });
        methods.add_function("md5", |_, data: String| Ok(hash::<Md5>(&data)));
        methods.add_function("sha1", |_, data: String| Ok(hash::<Sha1>(&data)));
        methods.add_function("sha256", |_, data: String| Ok(hash::<Sha256>(&data)));
        methods.add_function("sha384", |_, data: String| Ok(hash::<Sha384>(&data)));
        methods.add_function("sha512", |_, data: String| Ok(hash::<Sha512>(&data)));
        methods.add_function(
            "hmac",
            |_, (hash, data, secret): (String, String, String)| match hash.as_str() {
                "sha1" => hmac_hash::<Hmac<Sha1>>(&secret, &data),
                "sha256" => hmac_hash::<Hmac<Sha256>>(&secret, &data),
                "sha384" => hmac_hash::<Hmac<Sha384>>(&secret, &data),
                "sha512" => hmac_hash::<Hmac<Sha512>>(&secret, &data),
                _ => Err(bad_argument("hmac", 1, format!("unsupported hash {hash}"))),
            },
        );

        methods.add_function(
            "encrypt",
            |_, (cipher, data, key, iv): (String, String, String, Option<String>)| match cipher
                .as_str()
            {
                "aes-cbc" => {
                    let iv = iv.ok_or_else(|| bad_argument("encrypt", 4, "expect IV"))?;
                    let encrypted = Aes128CbcEnc::new(key.as_bytes().into(), iv.as_bytes().into())
                        .encrypt_padded_vec_mut::<Pkcs7>(data.as_bytes());
                    Ok(base16ct::lower::encode_string(&encrypted))
                }
                "des-cbc" => {
                    let iv = iv.ok_or_else(|| bad_argument("encrypt", 4, "expect IV"))?;
                    let encrypted = DesCbcEnc::new(key.as_bytes().into(), iv.as_bytes().into())
                        .encrypt_padded_vec_mut::<Pkcs7>(data.as_bytes());
                    Ok(base16ct::lower::encode_string(&encrypted))
                }
                "des-ecb" => {
                    let encrypted = DesEcbEnc::new(key.as_bytes().into())
                        .encrypt_padded_vec_mut::<Pkcs7>(data.as_bytes());
                    Ok(base16ct::lower::encode_string(&encrypted))
                }
                _ => Err(bad_argument(
                    "encrypt",
                    1,
                    format!("unsupported cipher {cipher}"),
                )),
            },
        );
        methods.add_function(
            "decrypt",
            |_, (cipher, encrypted, key, iv): (String, String, String, Option<String>)| match cipher
                .as_str()
            {
                "aes-cbc" => {
                    let iv = iv.ok_or_else(|| bad_argument("decrypt", 4, "expect IV"))?;
                    let data = hex::decode(&encrypted).into_lua_err()?;
                    let decrypted = Aes128CbcDec::new(key.as_bytes().into(), iv.as_bytes().into())
                        .decrypt_padded_vec_mut::<Pkcs7>(&data)
                        .map_err(|e| LuaError::external(e.to_string()))?;
                    Ok(String::from_utf8(decrypted).into_lua_err()?)
                }
                "des-cbc" => {
                    let iv = iv.ok_or_else(|| bad_argument("decrypt", 4, "expect IV"))?;
                    let data = hex::decode(&encrypted).into_lua_err()?;
                    let decrypted = DesCbcDec::new(key.as_bytes().into(), iv.as_bytes().into())
                        .decrypt_padded_vec_mut::<Pkcs7>(&data)
                        .map_err(|e| LuaError::external(e.to_string()))?;
                    Ok(String::from_utf8(decrypted).into_lua_err()?)
                }
                "des-ecb" => {
                    let data = hex::decode(&encrypted).into_lua_err()?;
                    let decrypted = DesEcbDec::new(key.as_bytes().into())
                        .decrypt_padded_vec_mut::<Pkcs7>(&data)
                        .map_err(|e| LuaError::external(e.to_string()))?;
                    Ok(String::from_utf8(decrypted).into_lua_err()?)
                }
                _ => Err(bad_argument(
                    "decrypt",
                    1,
                    format!("unsupported cipher {cipher}"),
                )),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::empty;

    use crate::Runner;

    #[tokio::test]
    async fn test_crypto() {
        let source = include_str!("../fixtures/bindings/crypto.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_crypto_errors() {
        let source = include_str!("../fixtures/bindings/crypto-errors.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_base64_roundtrip() {
        let source = include_str!("../fixtures/bindings/crypto/base64-roundtrip.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_hash_functions() {
        let source = include_str!("../fixtures/bindings/crypto/hash-functions.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_des_ecb_encryption() {
        let source = include_str!("../fixtures/bindings/crypto/des-ecb.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_des_cbc_encryption() {
        let source = include_str!("../fixtures/bindings/crypto/des-cbc.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}
