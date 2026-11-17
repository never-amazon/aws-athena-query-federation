use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::consts::{U16, U5};
use cbc::cipher::{BlockCipher, BlockDecryptMut, InnerIvInit, KeyInit, StreamCipher};
use des::{Des, TdesEde2, TdesEde3};
use md5::Digest;
use pkcs8::der::asn1::{AnyRef, OctetStringRef};
use pkcs8::der::Decode;
use pkcs8::ObjectIdentifier;
use rc2::Rc2;
use rc4::Rc4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pbes1Digest {
    Md2,
    Md5,
    Sha1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pbes1Cipher {
    Des,
    Rc2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pkcs12Cipher {
    Rc4 { key_len: usize },
    TripleDes { key_len: usize },
    Rc2 { key_len: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PbeScheme {
    Pbes1(Pbes1Digest, Pbes1Cipher),
    Pkcs12(Pkcs12Cipher),
}

const PBES1_SCHEMES: [(&str, PbeScheme); 5] = [
    (
        "1.2.840.113549.1.5.1",
        PbeScheme::Pbes1(Pbes1Digest::Md2, Pbes1Cipher::Des),
    ),
    (
        "1.2.840.113549.1.5.3",
        PbeScheme::Pbes1(Pbes1Digest::Md5, Pbes1Cipher::Des),
    ),
    (
        "1.2.840.113549.1.5.6",
        PbeScheme::Pbes1(Pbes1Digest::Md5, Pbes1Cipher::Rc2),
    ),
    (
        "1.2.840.113549.1.5.10",
        PbeScheme::Pbes1(Pbes1Digest::Sha1, Pbes1Cipher::Des),
    ),
    (
        "1.2.840.113549.1.5.11",
        PbeScheme::Pbes1(Pbes1Digest::Sha1, Pbes1Cipher::Rc2),
    ),
];

const PKCS12_SCHEMES: [(&str, PbeScheme); 6] = [
    (
        "1.2.840.113549.1.12.1.1",
        PbeScheme::Pkcs12(Pkcs12Cipher::Rc4 { key_len: 16 }),
    ),
    (
        "1.2.840.113549.1.12.1.2",
        PbeScheme::Pkcs12(Pkcs12Cipher::Rc4 { key_len: 5 }),
    ),
    (
        "1.2.840.113549.1.12.1.3",
        PbeScheme::Pkcs12(Pkcs12Cipher::TripleDes { key_len: 24 }),
    ),
    (
        "1.2.840.113549.1.12.1.4",
        PbeScheme::Pkcs12(Pkcs12Cipher::TripleDes { key_len: 16 }),
    ),
    (
        "1.2.840.113549.1.12.1.5",
        PbeScheme::Pkcs12(Pkcs12Cipher::Rc2 { key_len: 16 }),
    ),
    (
        "1.2.840.113549.1.12.1.6",
        PbeScheme::Pkcs12(Pkcs12Cipher::Rc2 { key_len: 5 }),
    ),
];

const SHA1_BLOCK_LEN: usize = 64;
const PKCS12_KEY_ID: u8 = 1;
const PKCS12_IV_ID: u8 = 2;
const BLOCK_IV_LEN: usize = 8;

impl PbeScheme {
    pub(crate) fn from_oid(oid: ObjectIdentifier) -> Option<PbeScheme> {
        let text = oid.to_string();
        PBES1_SCHEMES
            .iter()
            .chain(PKCS12_SCHEMES.iter())
            .find(|(candidate, _)| *candidate == text)
            .map(|(_, scheme)| *scheme)
    }

    pub(crate) fn decrypt(
        self,
        parameters: Option<AnyRef<'_>>,
        passphrase: &str,
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, String> {
        let parameters = parameters.ok_or_else(|| "missing PBE parameters".to_string())?;
        let (salt, iterations) = parameters
            .sequence(|reader| {
                let salt = OctetStringRef::decode(reader)?;
                let iterations = u32::decode(reader)?;
                Ok((salt.as_bytes().to_vec(), iterations))
            })
            .map_err(|e| e.to_string())?;
        match self {
            PbeScheme::Pbes1(digest, cipher) => {
                let derived = pbkdf1(digest, &ascii_password(passphrase), &salt, iterations);
                let (key, iv) = derived.split_at(BLOCK_IV_LEN);
                match cipher {
                    Pbes1Cipher::Des => cbc_decrypt(
                        Des::new_from_slice(key).map_err(|e| e.to_string())?,
                        iv,
                        ciphertext,
                    ),
                    Pbes1Cipher::Rc2 => cbc_decrypt(
                        Rc2::new_with_eff_key_len(key, key.len() * 8),
                        iv,
                        ciphertext,
                    ),
                }
            }
            PbeScheme::Pkcs12(cipher) => {
                let password = bmp_password(passphrase);
                let derive = |id: u8, len: usize| pkcs12_kdf(&password, &salt, iterations, id, len);
                match cipher {
                    Pkcs12Cipher::Rc4 { key_len } => {
                        let key = derive(PKCS12_KEY_ID, key_len);
                        let mut plaintext = ciphertext.to_vec();
                        if key_len == 5 {
                            Rc4::<U5>::new_from_slice(&key)
                                .map_err(|e| e.to_string())?
                                .apply_keystream(&mut plaintext);
                        } else {
                            Rc4::<U16>::new_from_slice(&key)
                                .map_err(|e| e.to_string())?
                                .apply_keystream(&mut plaintext);
                        }
                        Ok(plaintext)
                    }
                    Pkcs12Cipher::TripleDes { key_len } => {
                        let key = derive(PKCS12_KEY_ID, key_len);
                        let iv = derive(PKCS12_IV_ID, BLOCK_IV_LEN);
                        if key_len == 16 {
                            cbc_decrypt(
                                TdesEde2::new_from_slice(&key).map_err(|e| e.to_string())?,
                                &iv,
                                ciphertext,
                            )
                        } else {
                            cbc_decrypt(
                                TdesEde3::new_from_slice(&key).map_err(|e| e.to_string())?,
                                &iv,
                                ciphertext,
                            )
                        }
                    }
                    Pkcs12Cipher::Rc2 { key_len } => {
                        let key = derive(PKCS12_KEY_ID, key_len);
                        let iv = derive(PKCS12_IV_ID, BLOCK_IV_LEN);
                        cbc_decrypt(
                            Rc2::new_with_eff_key_len(&key, key_len * 8),
                            &iv,
                            ciphertext,
                        )
                    }
                }
            }
        }
    }
}

fn cbc_decrypt<C>(cipher: C, iv: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, String>
where
    C: BlockCipher + BlockDecryptMut,
{
    let mut buffer = ciphertext.to_vec();
    let plaintext_len = cbc::Decryptor::<C>::inner_iv_slice_init(cipher, iv)
        .map_err(|e| e.to_string())?
        .decrypt_padded_mut::<Pkcs7>(&mut buffer)
        .map_err(|e| e.to_string())?
        .len();
    buffer.truncate(plaintext_len);
    Ok(buffer)
}

fn ascii_password(passphrase: &str) -> Vec<u8> {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "BouncyCastle PasswordConverter.ASCII keeps the low byte of each UTF-16 unit"
    )]
    passphrase.encode_utf16().map(|unit| unit as u8).collect()
}

fn bmp_password(passphrase: &str) -> Vec<u8> {
    let mut bytes: Vec<u8> = passphrase
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

fn digest_bytes(digest: Pbes1Digest, data: &[u8]) -> Vec<u8> {
    match digest {
        Pbes1Digest::Md2 => md2::Md2::digest(data).to_vec(),
        Pbes1Digest::Md5 => md5::Md5::digest(data).to_vec(),
        Pbes1Digest::Sha1 => sha1::Sha1::digest(data).to_vec(),
    }
}

fn pbkdf1(digest: Pbes1Digest, password: &[u8], salt: &[u8], iterations: u32) -> Vec<u8> {
    let mut input = password.to_vec();
    input.extend_from_slice(salt);
    let mut derived = digest_bytes(digest, &input);
    for _ in 1..iterations {
        derived = digest_bytes(digest, &derived);
    }
    derived.truncate(2 * BLOCK_IV_LEN);
    derived
}

fn repeat_to_block_multiple(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let len = SHA1_BLOCK_LEN * data.len().div_ceil(SHA1_BLOCK_LEN);
    data.iter().copied().cycle().take(len).collect()
}

fn pkcs12_kdf(password: &[u8], salt: &[u8], iterations: u32, id: u8, len: usize) -> Vec<u8> {
    let diversifier = [id; SHA1_BLOCK_LEN];
    let mut input = repeat_to_block_multiple(salt);
    input.extend(repeat_to_block_multiple(password));
    let mut output = Vec::with_capacity(len);
    while output.len() < len {
        let mut hasher = sha1::Sha1::new();
        hasher.update(diversifier);
        hasher.update(&input);
        let mut block = hasher.finalize().to_vec();
        for _ in 1..iterations {
            block = sha1::Sha1::digest(&block).to_vec();
        }
        output.extend_from_slice(&block);
        let filler: Vec<u8> = block.iter().copied().cycle().take(SHA1_BLOCK_LEN).collect();
        for chunk in input.chunks_mut(SHA1_BLOCK_LEN) {
            let mut carry = 1u16;
            for (byte, add) in chunk.iter_mut().zip(filler.iter()).rev() {
                let sum = u16::from(*byte) + u16::from(*add) + carry;
                *byte = sum.to_le_bytes()[0];
                carry = sum >> 8;
            }
        }
    }
    output.truncate(len);
    output
}
