use aes::{Aes128, Aes192, Aes256};
use cbc::cipher::KeyInit;
use des::TdesEde3;
use pkcs8::der::asn1::{AnyRef, IntRef, OctetStringRef};
use pkcs8::der::{Decode, Reader};
use pkcs8::pkcs5::pbes2::{EncryptionScheme as Pbes2Cipher, SCRYPT_OID};
use pkcs8::pkcs5::{scrypt, EncryptionScheme};
use pkcs8::spki::AlgorithmIdentifierRef;
use pkcs8::ObjectIdentifier;

use super::pbe::{cbc_decrypt, PrivateKeyError, MAX_ITERATION_COUNT};

pub(crate) const PBES2_OID: ObjectIdentifier = pkcs8::pkcs5::pbes2::PBES2_OID;

const MAX_SCRYPT_BLOCK_SIZE: i32 = 1024;
const MAX_SCRYPT_MEMORY: i64 = 1_073_741_824;
const MAX_JAVA_INT_BITS: usize = 31;
const SCRYPT_CHUNK_WORDS: i32 = 1024;

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
struct ScryptCost {
    log_n: u8,
    r: u32,
    p: u32,
}

pub(crate) fn decrypt(
    parameters: Option<AnyRef<'_>>,
    passphrase: &str,
    ciphertext: &[u8],
) -> Result<Vec<u8>, PrivateKeyError> {
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
    let (salt, iterations) = kdf_parameters.sequence(|reader| {
        let salt = OctetStringRef::decode(reader)?;
        let iterations = IntRef::decode(reader)?;
        while !reader.is_finished() {
            AnyRef::decode(reader)?;
        }
        Ok((salt.as_bytes(), JavaBigInteger::from(iterations)))
    })?;
    check_pbkdf2_parameters(salt, iterations)?;
    let scheme = EncryptionScheme::try_from(AlgorithmIdentifierRef {
        oid: PBES2_OID,
        parameters: Some(parameters),
    })?;
    scheme
        .decrypt(passphrase.as_bytes(), ciphertext)
        .map_err(|error| pkcs8::Error::EncryptedPrivateKey(error).into())
}

fn check_pbkdf2_parameters(
    salt: &[u8],
    iterations: JavaBigInteger<'_>,
) -> Result<(), PrivateKeyError> {
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
    Ok(())
}

fn scrypt_decrypt(
    kdf_parameters: AnyRef<'_>,
    cipher: AlgorithmIdentifierRef<'_>,
    passphrase: &str,
    ciphertext: &[u8],
) -> Result<Vec<u8>, PrivateKeyError> {
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
    check_scrypt_memory(n, r)?;
    let cipher = Pbes2Cipher::try_from(cipher)?;
    let cost = check_scrypt_cost(n.int_value(), r.int_value(), p.int_value())?;
    let params = scrypt::Params::new(cost.log_n, cost.r, cost.p, scrypt::Params::RECOMMENDED_LEN)
        .map_err(|_| PrivateKeyError::ScryptKeyGeneration)?;
    let mut key = vec![0; cipher.key_size()];
    scrypt::scrypt(passphrase.as_bytes(), salt, &params, &mut key)
        .map_err(|_| PrivateKeyError::ScryptKeyGeneration)?;
    match cipher {
        Pbes2Cipher::Aes128Cbc { iv } => cbc_decrypt(Aes128::new_from_slice(&key)?, iv, ciphertext),
        Pbes2Cipher::Aes192Cbc { iv } => cbc_decrypt(Aes192::new_from_slice(&key)?, iv, ciphertext),
        Pbes2Cipher::Aes256Cbc { iv } => cbc_decrypt(Aes256::new_from_slice(&key)?, iv, ciphertext),
        Pbes2Cipher::DesEde3Cbc { iv } => {
            cbc_decrypt(TdesEde3::new_from_slice(&key)?, iv, ciphertext)
        }
        other => Err(PrivateKeyError::UnsupportedPbes2Cipher(other.oid())),
    }
}

fn check_scrypt_memory(
    n: JavaBigInteger<'_>,
    r: JavaBigInteger<'_>,
) -> Result<(), PrivateKeyError> {
    if !n.is_positive()
        || !r.is_positive()
        || n.bit_length() > MAX_JAVA_INT_BITS
        || r.bit_length() > MAX_JAVA_INT_BITS
    {
        return Err(PrivateKeyError::InvalidScryptParameters);
    }
    let r = r.int_value();
    if r > MAX_SCRYPT_BLOCK_SIZE {
        return Err(PrivateKeyError::ScryptBlockSizeTooLarge(r));
    }
    if i64::from(n.int_value()) > MAX_SCRYPT_MEMORY / (128 * i64::from(r)) {
        return Err(PrivateKeyError::ScryptMemoryTooLarge);
    }
    Ok(())
}

fn check_scrypt_cost(n: i32, r: i32, p: i32) -> Result<ScryptCost, PrivateKeyError> {
    if n <= 1 {
        return Err(PrivateKeyError::ScryptCostTooSmall);
    }
    if n.count_ones() != 1 {
        return Err(PrivateKeyError::ScryptCostNotPowerOfTwo);
    }
    let log_n =
        u8::try_from(n.trailing_zeros()).map_err(|_| PrivateKeyError::ScryptCostNotPowerOfTwo)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn integer(bytes: &[u8]) -> JavaBigInteger<'_> {
        JavaBigInteger::from(IntRef::new(bytes).unwrap())
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
        assert!(check_pbkdf2_parameters(&salt, integer(&[0x00, 0x98, 0x96, 0x80])).is_ok());
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
    fn scrypt_memory_follows_bouncy_castle_limits() {
        let r_1024 = [0x04, 0x00];
        assert!(check_scrypt_memory(integer(&[0x20, 0x00]), integer(&r_1024)).is_ok());
        assert!(matches!(
            check_scrypt_memory(integer(&[0x40, 0x00]), integer(&r_1024)),
            Err(PrivateKeyError::ScryptMemoryTooLarge)
        ));
        assert!(matches!(
            check_scrypt_memory(integer(&[2]), integer(&[0x04, 0x01])),
            Err(PrivateKeyError::ScryptBlockSizeTooLarge(1025))
        ));
        assert!(matches!(
            check_scrypt_memory(integer(&[0]), integer(&[8])),
            Err(PrivateKeyError::InvalidScryptParameters)
        ));
        assert!(matches!(
            check_scrypt_memory(integer(&[0x00, 0x80, 0, 0, 0]), integer(&[8])),
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
            check_scrypt_cost(1024, 8, 1).unwrap()
        );
        assert_eq!(
            ScryptCost {
                log_n: 1,
                r: 1024,
                p: 2
            },
            check_scrypt_cost(2, 1024, 2).unwrap()
        );
        assert!(check_scrypt_cost(1024, 512, 1).is_ok());
        assert!(check_scrypt_cost(8, 8, 262_143).is_ok());
        assert!(matches!(
            check_scrypt_cost(1, 1, 1),
            Err(PrivateKeyError::ScryptCostTooSmall)
        ));
        assert!(matches!(
            check_scrypt_cost(3, 8, 1),
            Err(PrivateKeyError::ScryptCostNotPowerOfTwo)
        ));
        assert!(matches!(
            check_scrypt_cost(65_536, 1, 1),
            Err(PrivateKeyError::ScryptCostTooLargeForBlockSizeOne)
        ));
        assert!(matches!(
            check_scrypt_cost(2, 8, 262_144),
            Err(PrivateKeyError::ScryptParallelization {
                max_p: 262_143,
                r: 8
            })
        ));
        assert!(matches!(
            check_scrypt_cost(2, 8, 0),
            Err(PrivateKeyError::ScryptParallelization {
                max_p: 262_143,
                r: 8
            })
        ));
        assert!(matches!(
            check_scrypt_cost(4, 513, 1),
            Err(PrivateKeyError::ScryptKeyGeneration)
        ));
        assert!(matches!(
            check_scrypt_cost(8192, 1024, 1),
            Err(PrivateKeyError::ScryptKeyGeneration)
        ));
    }
}
