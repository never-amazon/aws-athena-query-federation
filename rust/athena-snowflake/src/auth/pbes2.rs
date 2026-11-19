use aes::{Aes128, Aes192, Aes256};
use cbc::cipher::KeyInit;
use des::TdesEde3;
use gost94::Gost94CryptoPro;
use hmac::Hmac;
use pkcs8::der::asn1::{AnyRef, IntRef, OctetStringRef};
use pkcs8::der::{Decode, Reader, Tag, Tagged};
use pkcs8::pkcs5::pbes2::{EncryptionScheme, PBKDF2_OID, SCRYPT_OID};
use pkcs8::spki::AlgorithmIdentifierRef;
use pkcs8::ObjectIdentifier;
use sha1::Sha1;
use sha2::{Sha224, Sha256, Sha384, Sha512};
use sha3::{Sha3_224, Sha3_256, Sha3_384, Sha3_512};
use sm3::Sm3;
use sm4::Sm4;
use zeroize::Zeroizing;

use super::pbe::{ascii_password, cbc_decrypt, PrivateKeyError, MAX_ITERATION_COUNT};
use scrypt_cost::ScryptMemory;

pub(crate) const PBES2_OID: ObjectIdentifier = pkcs8::pkcs5::pbes2::PBES2_OID;

const SM4_CBC_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.156.10197.1.104.2");
const MAX_JAVA_INT_BITS: usize = 31;

const PRFS: [(&str, Prf); 11] = [
    ("1.2.840.113549.2.7", Prf::HmacSha1),
    ("1.2.840.113549.2.8", Prf::HmacSha224),
    ("1.2.840.113549.2.9", Prf::HmacSha256),
    ("1.2.840.113549.2.10", Prf::HmacSha384),
    ("1.2.840.113549.2.11", Prf::HmacSha512),
    ("2.16.840.1.101.3.4.2.13", Prf::HmacSha3_224),
    ("2.16.840.1.101.3.4.2.14", Prf::HmacSha3_256),
    ("2.16.840.1.101.3.4.2.15", Prf::HmacSha3_384),
    ("2.16.840.1.101.3.4.2.16", Prf::HmacSha3_512),
    ("1.2.643.2.2.10", Prf::HmacGost3411),
    ("1.2.156.10197.1.401.2", Prf::HmacSm3),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JavaBigInteger<'a>(&'a [u8]);

impl<'a> From<IntRef<'a>> for JavaBigInteger<'a> {
    fn from(value: IntRef<'a>) -> Self {
        JavaBigInteger(value.as_bytes())
    }
}

impl JavaBigInteger<'_> {
    fn is_negative(self) -> bool {
        self.0.first().is_some_and(|byte| byte & 0x80 != 0)
    }

    fn is_positive(self) -> bool {
        !self.is_negative() && self.0.iter().any(|byte| *byte != 0)
    }

    fn bit_length(self) -> usize {
        let magnitude: &[u8] = match self.0.iter().position(|byte| *byte != 0) {
            Some(start) => &self.0[start..],
            None => &[],
        };
        magnitude.first().map_or(0, |first| {
            magnitude.len() * 8 - first.leading_zeros() as usize
        })
    }

    fn int_value(self) -> i32 {
        const WORD_LEN: usize = 4;
        let mut word = if self.is_negative() {
            [0xff; WORD_LEN]
        } else {
            [0; WORD_LEN]
        };
        let low = &self.0[self.0.len().saturating_sub(WORD_LEN)..];
        word[WORD_LEN - low.len()..].copy_from_slice(low);
        i32::from_be_bytes(word)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prf {
    HmacSha1,
    HmacSha224,
    HmacSha256,
    HmacSha384,
    HmacSha512,
    HmacSha3_224,
    HmacSha3_256,
    HmacSha3_384,
    HmacSha3_512,
    HmacGost3411,
    HmacSm3,
}

impl Prf {
    fn from_algorithm(prf: Option<AlgorithmIdentifierRef<'_>>) -> Result<Prf, PrivateKeyError> {
        let Some(prf) = prf else {
            return Ok(Prf::HmacSha1);
        };
        let text = prf.oid.to_string();
        PRFS.iter()
            .find(|(oid, _)| *oid == text)
            .map(|(_, prf)| *prf)
            .ok_or(PrivateKeyError::UnknownPrf(prf.oid))
    }

    fn derive(
        self,
        passphrase: &str,
        salt: &[u8],
        iterations: u32,
        key: &mut [u8],
    ) -> Result<(), PrivateKeyError> {
        let utf8 = passphrase.as_bytes();
        match self {
            Prf::HmacSha1 => {
                pbkdf2::pbkdf2::<Hmac<Sha1>>(&ascii_password(passphrase), salt, iterations, key)
            }
            Prf::HmacSha224 => pbkdf2::pbkdf2::<Hmac<Sha224>>(utf8, salt, iterations, key),
            Prf::HmacSha256 => pbkdf2::pbkdf2::<Hmac<Sha256>>(utf8, salt, iterations, key),
            Prf::HmacSha384 => pbkdf2::pbkdf2::<Hmac<Sha384>>(utf8, salt, iterations, key),
            Prf::HmacSha512 => pbkdf2::pbkdf2::<Hmac<Sha512>>(utf8, salt, iterations, key),
            Prf::HmacSha3_224 => pbkdf2::pbkdf2::<Hmac<Sha3_224>>(utf8, salt, iterations, key),
            Prf::HmacSha3_256 => pbkdf2::pbkdf2::<Hmac<Sha3_256>>(utf8, salt, iterations, key),
            Prf::HmacSha3_384 => pbkdf2::pbkdf2::<Hmac<Sha3_384>>(utf8, salt, iterations, key),
            Prf::HmacSha3_512 => pbkdf2::pbkdf2::<Hmac<Sha3_512>>(utf8, salt, iterations, key),
            Prf::HmacGost3411 => {
                pbkdf2::pbkdf2::<Hmac<Gost94CryptoPro>>(utf8, salt, iterations, key)
            }
            Prf::HmacSm3 => pbkdf2::pbkdf2::<Hmac<Sm3>>(utf8, salt, iterations, key),
        }
        .map_err(PrivateKeyError::Pbkdf2KeyLength)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pbkdf2Params<'a> {
    salt: &'a [u8],
    iterations: JavaBigInteger<'a>,
    prf: Option<AlgorithmIdentifierRef<'a>>,
}

impl<'a> Pbkdf2Params<'a> {
    fn decode(parameters: AnyRef<'a>) -> Result<Self, PrivateKeyError> {
        let params = parameters.sequence(|reader| {
            let salt = OctetStringRef::decode(reader)?;
            let iterations = IntRef::decode(reader)?;
            let mut next = Option::<AnyRef<'a>>::decode(reader)?;
            if let Some(key_length) = next.filter(|any| any.tag() == Tag::Integer) {
                IntRef::try_from(key_length)?;
                next = Option::<AnyRef<'a>>::decode(reader)?;
            }
            let prf = next
                .map(AnyRef::decode_as::<AlgorithmIdentifierRef<'a>>)
                .transpose()?;
            while !reader.is_finished() {
                AnyRef::decode(reader)?;
            }
            Ok(Pbkdf2Params {
                salt: salt.as_bytes(),
                iterations: JavaBigInteger::from(iterations),
                prf,
            })
        })?;
        Ok(params)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CbcCipher<'a> {
    Aes128(&'a [u8]),
    Aes192(&'a [u8]),
    Aes256(&'a [u8]),
    DesEde3(&'a [u8]),
    Sm4(&'a [u8]),
}

impl<'a> CbcCipher<'a> {
    fn from_algorithm(algorithm: AlgorithmIdentifierRef<'a>) -> Result<Self, PrivateKeyError> {
        if algorithm.oid == SM4_CBC_OID {
            let iv = algorithm
                .parameters
                .ok_or(PrivateKeyError::MissingPbeParameters)?
                .decode_as::<OctetStringRef<'a>>()?;
            return Ok(CbcCipher::Sm4(iv.as_bytes()));
        }
        match EncryptionScheme::try_from(algorithm)? {
            EncryptionScheme::Aes128Cbc { iv } => Ok(CbcCipher::Aes128(iv)),
            EncryptionScheme::Aes192Cbc { iv } => Ok(CbcCipher::Aes192(iv)),
            EncryptionScheme::Aes256Cbc { iv } => Ok(CbcCipher::Aes256(iv)),
            EncryptionScheme::DesEde3Cbc { iv } => Ok(CbcCipher::DesEde3(iv)),
            other => Err(PrivateKeyError::UnsupportedPbes2Cipher(other.oid())),
        }
    }

    fn key_size(self) -> usize {
        match self {
            CbcCipher::Aes128(_) | CbcCipher::Sm4(_) => 16,
            CbcCipher::Aes192(_) | CbcCipher::DesEde3(_) => 24,
            CbcCipher::Aes256(_) => 32,
        }
    }

    fn decrypt(self, key: &[u8], ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>, PrivateKeyError> {
        match self {
            CbcCipher::Aes128(iv) => cbc_decrypt(Aes128::new_from_slice(key)?, iv, ciphertext),
            CbcCipher::Aes192(iv) => cbc_decrypt(Aes192::new_from_slice(key)?, iv, ciphertext),
            CbcCipher::Aes256(iv) => cbc_decrypt(Aes256::new_from_slice(key)?, iv, ciphertext),
            CbcCipher::DesEde3(iv) => cbc_decrypt(TdesEde3::new_from_slice(key)?, iv, ciphertext),
            CbcCipher::Sm4(iv) => cbc_decrypt(Sm4::new_from_slice(key)?, iv, ciphertext),
        }
    }
}

pub(crate) fn decrypt(
    parameters: Option<AnyRef<'_>>,
    passphrase: &str,
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>, PrivateKeyError> {
    let parameters = parameters.ok_or(PrivateKeyError::MissingPbeParameters)?;
    let (kdf, cipher) = parameters.sequence(|reader| {
        let kdf = AlgorithmIdentifierRef::decode(reader)?;
        let cipher = AlgorithmIdentifierRef::decode(reader)?;
        Ok((kdf, cipher))
    })?;
    let kdf_parameters = kdf
        .parameters
        .ok_or(PrivateKeyError::MissingPbeParameters)?;
    if kdf.oid == SCRYPT_OID {
        return scrypt_decrypt(kdf_parameters, cipher, passphrase, ciphertext);
    }
    if kdf.oid != PBKDF2_OID {
        return Err(PrivateKeyError::UnsupportedKdf(kdf.oid));
    }
    let params = Pbkdf2Params::decode(kdf_parameters)?;
    let iterations = check_pbkdf2_parameters(params.salt, params.iterations)?;
    let cipher = CbcCipher::from_algorithm(cipher)?;
    let prf = Prf::from_algorithm(params.prf)?;
    let mut key = Zeroizing::new(vec![0; cipher.key_size()]);
    prf.derive(passphrase, params.salt, iterations, &mut key)?;
    cipher.decrypt(&key, ciphertext)
}

fn check_pbkdf2_parameters(
    salt: &[u8],
    iterations: JavaBigInteger<'_>,
) -> Result<u32, PrivateKeyError> {
    if iterations.is_negative() || iterations.bit_length() > MAX_JAVA_INT_BITS {
        return Err(PrivateKeyError::InvalidIterationCount);
    }
    let iterations = iterations.int_value().unsigned_abs();
    if iterations > MAX_ITERATION_COUNT {
        return Err(PrivateKeyError::IterationCountTooLarge(iterations));
    }
    if salt.is_empty() {
        return Err(PrivateKeyError::EmptySalt);
    }
    if iterations == 0 {
        return Err(PrivateKeyError::IterationCountNotPositive);
    }
    Ok(iterations)
}

fn scrypt_decrypt(
    kdf_parameters: AnyRef<'_>,
    cipher: AlgorithmIdentifierRef<'_>,
    passphrase: &str,
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>, PrivateKeyError> {
    let (salt, n, r, p) = kdf_parameters.sequence(|reader| {
        let salt = OctetStringRef::decode(reader)?;
        let n = IntRef::decode(reader)?;
        let r = IntRef::decode(reader)?;
        let p = IntRef::decode(reader)?;
        Option::<IntRef<'_>>::decode(reader)?;
        Ok((
            salt.as_bytes(),
            JavaBigInteger::from(n),
            JavaBigInteger::from(r),
            JavaBigInteger::from(p),
        ))
    })?;
    let memory = ScryptMemory::new(n, r)?;
    let cipher = CbcCipher::from_algorithm(cipher)?;
    let cost = memory.cost(p.int_value())?;
    let params = scrypt::Params::new(cost.log_n, cost.r, cost.p, scrypt::Params::RECOMMENDED_LEN)
        .map_err(PrivateKeyError::ScryptParams)?;
    let mut key = Zeroizing::new(vec![0; cipher.key_size()]);
    scrypt::scrypt(passphrase.as_bytes(), salt, &params, &mut key)
        .map_err(PrivateKeyError::ScryptOutputLength)?;
    cipher.decrypt(&key, ciphertext)
}

mod scrypt_cost {
    use super::{JavaBigInteger, PrivateKeyError, MAX_JAVA_INT_BITS};

    const MAX_SCRYPT_BLOCK_SIZE: i32 = 1024;
    const MAX_SCRYPT_MEMORY: i64 = 1_073_741_824;
    const SCRYPT_CHUNK_WORDS: i32 = 1024;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct ScryptCost {
        pub(super) log_n: u8,
        pub(super) r: u32,
        pub(super) p: u32,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct ScryptMemory {
        n: i32,
        r: i32,
    }

    impl ScryptMemory {
        pub(super) fn new(
            n: JavaBigInteger<'_>,
            r: JavaBigInteger<'_>,
        ) -> Result<Self, PrivateKeyError> {
            if !n.is_positive()
                || !r.is_positive()
                || n.bit_length() > MAX_JAVA_INT_BITS
                || r.bit_length() > MAX_JAVA_INT_BITS
            {
                return Err(PrivateKeyError::InvalidScryptParameters);
            }
            let (n, r) = (n.int_value(), r.int_value());
            if r > MAX_SCRYPT_BLOCK_SIZE {
                return Err(PrivateKeyError::ScryptBlockSizeTooLarge(r));
            }
            if i64::from(n) > MAX_SCRYPT_MEMORY / (128 * i64::from(r)) {
                return Err(PrivateKeyError::ScryptMemoryTooLarge);
            }
            Ok(ScryptMemory { n, r })
        }

        pub(super) fn cost(self, p: i32) -> Result<ScryptCost, PrivateKeyError> {
            let ScryptMemory { n, r } = self;
            if n <= 1 {
                return Err(PrivateKeyError::ScryptCostTooSmall);
            }
            if n.count_ones() != 1 {
                return Err(PrivateKeyError::ScryptCostNotPowerOfTwo);
            }
            let log_n = u8::try_from(n.trailing_zeros())
                .map_err(|_| PrivateKeyError::ScryptCostNotPowerOfTwo)?;
            if r == 1 && n >= 65_536 {
                return Err(PrivateKeyError::ScryptCostTooLargeForBlockSizeOne);
            }
            let max_p = i32::MAX / (128 * r * 8);
            if p < 1 || p > max_p {
                return Err(PrivateKeyError::ScryptParallelization { max_p, r });
            }
            let mut chunk_shift = 0;
            let mut words = n * r;
            while n - chunk_shift > 2 && words > SCRYPT_CHUNK_WORDS {
                chunk_shift += 1;
                words >>= 1;
            }
            if chunk_shift >= i32::from(log_n) {
                return Err(PrivateKeyError::ScryptKeyGeneration);
            }
            Ok(ScryptCost {
                log_n,
                r: r.unsigned_abs(),
                p: p.unsigned_abs(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::scrypt_cost::ScryptCost;
    use super::*;

    fn integer(bytes: &[u8]) -> JavaBigInteger<'_> {
        JavaBigInteger::from(IntRef::new(bytes).unwrap())
    }

    fn int_bytes(value: i32) -> Vec<u8> {
        let bytes = value.to_be_bytes();
        let start = (0..3)
            .find(|&i| !(bytes[i] == 0 && bytes[i + 1] & 0x80 == 0))
            .unwrap_or(3);
        bytes[start..].to_vec()
    }

    fn cost(n: i32, r: i32, p: i32) -> Result<ScryptCost, PrivateKeyError> {
        let (n, r) = (int_bytes(n), int_bytes(r));
        ScryptMemory::new(integer(&n), integer(&r))?.cost(p)
    }

    #[test]
    fn java_big_integer_matches_java_semantics() {
        assert_eq!(0, integer(&[0]).bit_length());
        assert!(!integer(&[0]).is_positive());
        assert_eq!(31, integer(&[0x7f, 0xff, 0xff, 0xff]).bit_length());
        assert_eq!(32, integer(&[0, 0x80, 0, 0, 0]).bit_length());
        assert!(integer(&[0xff]).is_negative());
        assert_eq!(-1, integer(&[0xff]).int_value());
        assert_eq!(1, integer(&[1, 0, 0, 0, 1]).int_value());
        assert_eq!(i32::MIN, integer(&[0, 0x80, 0, 0, 0]).int_value());
    }

    #[test]
    fn pbkdf2_iteration_count_follows_bouncy_castle_limits() {
        let salt = [1u8; 8];
        assert_eq!(
            10_000_000,
            check_pbkdf2_parameters(&salt, integer(&[0x00, 0x98, 0x96, 0x80])).unwrap()
        );
        assert!(matches!(
            check_pbkdf2_parameters(&salt, integer(&[0x00, 0x98, 0x96, 0x81])),
            Err(PrivateKeyError::IterationCountTooLarge(10_000_001))
        ));
        assert!(matches!(
            check_pbkdf2_parameters(&salt, integer(&[0x7f, 0xff, 0xff, 0xff])),
            Err(PrivateKeyError::IterationCountTooLarge(2_147_483_647))
        ));
        assert!(matches!(
            check_pbkdf2_parameters(&salt, integer(&[0x00, 0x80, 0, 0, 0])),
            Err(PrivateKeyError::InvalidIterationCount)
        ));
        assert!(matches!(
            check_pbkdf2_parameters(&salt, integer(&[0xff])),
            Err(PrivateKeyError::InvalidIterationCount)
        ));
        assert!(matches!(
            check_pbkdf2_parameters(&[], integer(&[0x08, 0])),
            Err(PrivateKeyError::EmptySalt)
        ));
        assert!(matches!(
            check_pbkdf2_parameters(&salt, integer(&[0])),
            Err(PrivateKeyError::IterationCountNotPositive)
        ));
    }

    #[test]
    fn pbkdf2_prf_follows_bouncy_castle_table() {
        let algorithm = |oid: &'static str| AlgorithmIdentifierRef {
            oid: ObjectIdentifier::new_unwrap(oid),
            parameters: None,
        };
        assert_eq!(Prf::HmacSha1, Prf::from_algorithm(None).unwrap());
        for (oid, prf) in PRFS {
            assert_eq!(prf, Prf::from_algorithm(Some(algorithm(oid))).unwrap());
        }
        for unknown in [
            "1.2.840.113549.2.6",
            "1.2.840.113549.2.12",
            "1.2.840.113549.2.13",
        ] {
            assert!(matches!(
                Prf::from_algorithm(Some(algorithm(unknown))),
                Err(PrivateKeyError::UnknownPrf(_))
            ));
        }
    }

    #[test]
    fn pbkdf2_sha1_uses_the_low_byte_of_each_utf16_unit() {
        let mut eight_bit = [0u8; 20];
        let mut expected = [0u8; 20];
        Prf::HmacSha1
            .derive("pä€😀", b"salt", 2, &mut eight_bit)
            .unwrap();
        pbkdf2::pbkdf2::<Hmac<Sha1>>(&[0x70, 0xe4, 0xac, 0x3d, 0x00], b"salt", 2, &mut expected)
            .unwrap();
        assert_eq!(expected, eight_bit);
        let mut utf8 = [0u8; 20];
        let mut expected = [0u8; 20];
        Prf::HmacSha256
            .derive("pä€😀", b"salt", 2, &mut utf8)
            .unwrap();
        pbkdf2::pbkdf2::<Hmac<Sha256>>("pä€😀".as_bytes(), b"salt", 2, &mut expected).unwrap();
        assert_eq!(expected, utf8);
    }

    #[test]
    fn pbkdf2_params_follow_bouncy_castle_parsing() {
        fn decode(der: &[u8]) -> Result<Pbkdf2Params<'_>, PrivateKeyError> {
            Pbkdf2Params::decode(AnyRef::from_der(der).unwrap())
        }
        fn sequence(parts: &[&[u8]]) -> Vec<u8> {
            let body: Vec<u8> = parts.concat();
            let mut der = vec![0x30, u8::try_from(body.len()).unwrap()];
            der.extend(body);
            der
        }
        let salt_iter: &[u8] = &[0x04, 0x01, 0xaa, 0x02, 0x01, 0x02];
        let sha256: &[u8] = &[
            0x30, 0x0c, 0x06, 0x08, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x02, 0x09, 0x05, 0x00,
        ];
        let key_length: &[u8] = &[0x02, 0x01, 0x10];
        let only_salt = sequence(&[salt_iter]);
        let params = decode(&only_salt).unwrap();
        assert_eq!((&[0xaa][..], None), (params.salt, params.prf));
        let full = sequence(&[salt_iter, key_length, sha256, &[0x05, 0x00]]);
        let params = decode(&full).unwrap();
        assert_eq!("1.2.840.113549.2.9", params.prf.unwrap().oid.to_string());
        let negative_key_length = sequence(&[salt_iter, &[0x02, 0x01, 0xff]]);
        assert!(decode(&negative_key_length).unwrap().prf.is_none());
        assert!(decode(&sequence(&[salt_iter, &[0x05, 0x00]])).is_err());
        assert!(decode(&sequence(&[salt_iter, key_length, key_length])).is_err());
        assert!(decode(&sequence(&[salt_iter, &[0x30, 0x00]])).is_err());
    }

    #[test]
    fn scrypt_memory_follows_bouncy_castle_limits() {
        let r_1024 = [0x04, 0x00];
        assert!(ScryptMemory::new(integer(&[0x20, 0x00]), integer(&r_1024)).is_ok());
        assert!(matches!(
            ScryptMemory::new(integer(&[0x40, 0x00]), integer(&r_1024)),
            Err(PrivateKeyError::ScryptMemoryTooLarge)
        ));
        assert!(matches!(
            ScryptMemory::new(integer(&[2]), integer(&[0x04, 0x01])),
            Err(PrivateKeyError::ScryptBlockSizeTooLarge(1025))
        ));
        assert!(matches!(
            ScryptMemory::new(integer(&[0]), integer(&[8])),
            Err(PrivateKeyError::InvalidScryptParameters)
        ));
        assert!(matches!(
            ScryptMemory::new(integer(&[2]), integer(&[0])),
            Err(PrivateKeyError::InvalidScryptParameters)
        ));
        assert!(matches!(
            ScryptMemory::new(integer(&[0x00, 0x80, 0, 0, 0]), integer(&[8])),
            Err(PrivateKeyError::InvalidScryptParameters)
        ));
    }

    #[test]
    fn scrypt_cost_follows_bouncy_castle_limits() {
        assert_eq!(
            ScryptCost {
                log_n: 10,
                r: 8,
                p: 1
            },
            cost(1024, 8, 1).unwrap()
        );
        assert_eq!(
            ScryptCost {
                log_n: 1,
                r: 1024,
                p: 2
            },
            cost(2, 1024, 2).unwrap()
        );
        assert!(cost(1024, 512, 1).is_ok());
        assert!(cost(8, 8, 262_143).is_ok());
        assert!(cost(2, 1024, 2047).is_ok());
        assert!(matches!(
            cost(1, 1, 1),
            Err(PrivateKeyError::ScryptCostTooSmall)
        ));
        assert!(matches!(
            cost(3, 8, 1),
            Err(PrivateKeyError::ScryptCostNotPowerOfTwo)
        ));
        assert!(matches!(
            cost(65_536, 1, 1),
            Err(PrivateKeyError::ScryptCostTooLargeForBlockSizeOne)
        ));
        assert!(matches!(
            cost(2, 8, 262_144),
            Err(PrivateKeyError::ScryptParallelization {
                max_p: 262_143,
                r: 8
            })
        ));
        assert!(matches!(
            cost(2, 8, 0),
            Err(PrivateKeyError::ScryptParallelization {
                max_p: 262_143,
                r: 8
            })
        ));
        assert!(matches!(
            cost(4, 513, 1),
            Err(PrivateKeyError::ScryptKeyGeneration)
        ));
        assert!(matches!(
            cost(8192, 1024, 1),
            Err(PrivateKeyError::ScryptKeyGeneration)
        ));
    }
}
