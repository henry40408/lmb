use std::{fmt, sync::Arc};

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut as _, BlockEncryptMut as _};
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
        let source = include_str!("fixtures/crypto.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}
