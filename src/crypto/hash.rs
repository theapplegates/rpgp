use std::str::FromStr;

use digest::DynDigest;
use md5::Md5;
use num_enum::{FromPrimitive, IntoPrimitive};
use ripemd::Ripemd160;
use sha1_checked::{CollisionResult, Sha1};

use crate::errors::{Error, Result};

/// Available hash algorithms.
/// Ref: <https://www.rfc-editor.org/rfc/rfc9580.html#name-hash-algorithms>
#[derive(
    Debug, PartialEq, Eq, Copy, Clone, FromPrimitive, IntoPrimitive, Hash, derive_more::Display,
)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
#[repr(u8)]
pub enum HashAlgorithm {
    #[cfg_attr(test, proptest(skip))]
    #[display("NONE")]
    None = 0,
    #[display("MD5")]
    MD5 = 1,
    #[display("SHA1")]
    SHA1 = 2,
    #[display("RIPEMD160")]
    RIPEMD160 = 3,

    #[display("SHA256")]
    SHA2_256 = 8,
    #[display("SHA384")]
    SHA2_384 = 9,
    #[display("SHA512")]
    SHA2_512 = 10,
    #[display("SHA224")]
    SHA2_224 = 11,
    #[display("SHA3-256")]
    SHA3_256 = 12,
    #[display("SHA3-512")]
    SHA3_512 = 14,

    /// Do not use, just for compatibility with GnuPG.
    Private10 = 110,

    #[num_enum(catch_all)]
    Other(#[cfg_attr(test, proptest(strategy = "111u8.."))] u8),
}

impl Default for HashAlgorithm {
    fn default() -> Self {
        Self::SHA2_256
    }
}

impl FromStr for HashAlgorithm {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "none" => Ok(Self::None),
            "md5" => Ok(Self::MD5),
            "sha1" => Ok(Self::SHA1),
            "ripemd160" => Ok(Self::RIPEMD160),
            "sha256" => Ok(Self::SHA2_256),
            "sha384" => Ok(Self::SHA2_384),
            "sha512" => Ok(Self::SHA2_512),
            "sha224" => Ok(Self::SHA2_224),
            "sha3-256" => Ok(Self::SHA3_256),
            "sha3-512" => Ok(Self::SHA3_512),
            "private10" => Ok(Self::Private10),
            _ => bail!("unknown hash"),
        }
    }
}

impl HashAlgorithm {
    /// V6 signature salt size
    /// <https://www.rfc-editor.org/rfc/rfc9580.html#hash-algos>
    pub const fn salt_len(&self) -> Option<usize> {
        match self {
            Self::SHA2_224 => Some(16),
            Self::SHA2_256 => Some(16),
            Self::SHA2_384 => Some(24),
            Self::SHA2_512 => Some(32),
            Self::SHA3_256 => Some(16),
            Self::SHA3_512 => Some(32),
            _ => None,
        }
    }
}

impl zeroize::DefaultIsZeroes for HashAlgorithm {}

/// Temporary wrapper around `Box<dyn DynDigest>` to implement `io::Write`.
pub(crate) struct WriteHasher<'a>(pub(crate) &'a mut Box<dyn DynDigest>);

impl std::io::Write for WriteHasher<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let digest = &mut **self.0;
        DynDigest::update(digest, buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl HashAlgorithm {
    /// Create a new hasher.
    pub fn new_hasher(self) -> Result<Box<dyn DynDigest>> {
        match self {
            HashAlgorithm::MD5 => Ok(Box::<Md5>::default()),
            HashAlgorithm::SHA1 => Ok(Box::<Sha1>::default()),
            HashAlgorithm::RIPEMD160 => Ok(Box::<Ripemd160>::default()),
            HashAlgorithm::SHA2_256 => Ok(Box::<sha2::Sha256>::default()),
            HashAlgorithm::SHA2_384 => Ok(Box::<sha2::Sha384>::default()),
            HashAlgorithm::SHA2_512 => Ok(Box::<sha2::Sha512>::default()),
            HashAlgorithm::SHA2_224 => Ok(Box::<sha2::Sha224>::default()),
            HashAlgorithm::SHA3_256 => Ok(Box::<sha3::Sha3_256>::default()),
            HashAlgorithm::SHA3_512 => Ok(Box::<sha3::Sha3_512>::default()),
            _ => unimplemented_err!("hasher {:?}", self),
        }
    }

    /// Calculate the digest of the given input data.
    pub fn digest(self, data: &[u8]) -> Result<Vec<u8>> {
        use digest::Digest;

        Ok(match self {
            HashAlgorithm::MD5 => Md5::digest(data).to_vec(),
            HashAlgorithm::SHA1 => match Sha1::try_digest(data) {
                CollisionResult::Ok(output) => output.to_vec(),
                CollisionResult::Collision(_) | CollisionResult::Mitigated(_) => {
                    return Err(Error::Sha1HashCollision)
                }
            },
            HashAlgorithm::RIPEMD160 => Ripemd160::digest(data).to_vec(),
            HashAlgorithm::SHA2_256 => sha2::Sha256::digest(data).to_vec(),
            HashAlgorithm::SHA2_384 => sha2::Sha384::digest(data).to_vec(),
            HashAlgorithm::SHA2_512 => sha2::Sha512::digest(data).to_vec(),
            HashAlgorithm::SHA2_224 => sha2::Sha224::digest(data).to_vec(),
            HashAlgorithm::SHA3_256 => sha3::Sha3_256::digest(data).to_vec(),
            HashAlgorithm::SHA3_512 => sha3::Sha3_512::digest(data).to_vec(),

            HashAlgorithm::Private10 => unsupported_err!("Private10 should not be used"),
            _ => unimplemented_err!("hasher: {:?}", self),
        })
    }

    /// Returns the expected digest size for the given algorithm.
    pub fn digest_size(self) -> Option<usize> {
        use digest::Digest;

        let size = match self {
            HashAlgorithm::MD5 => <Md5 as Digest>::output_size(),
            HashAlgorithm::SHA1 => <Sha1 as Digest>::output_size(),
            HashAlgorithm::RIPEMD160 => <Ripemd160 as Digest>::output_size(),
            HashAlgorithm::SHA2_256 => <sha2::Sha256 as Digest>::output_size(),
            HashAlgorithm::SHA2_384 => <sha2::Sha384 as Digest>::output_size(),
            HashAlgorithm::SHA2_512 => <sha2::Sha512 as Digest>::output_size(),
            HashAlgorithm::SHA2_224 => <sha2::Sha224 as Digest>::output_size(),
            HashAlgorithm::SHA3_256 => <sha3::Sha3_256 as Digest>::output_size(),
            HashAlgorithm::SHA3_512 => <sha3::Sha3_512 as Digest>::output_size(),
            _ => return None,
        };
        Some(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_parse_hash() {
        assert_eq!(HashAlgorithm::None.to_string(), "NONE".to_string());

        assert_eq!(HashAlgorithm::SHA2_256.to_string(), "SHA256".to_string());

        assert_eq!(HashAlgorithm::SHA3_512, "SHA3-512".parse().unwrap());
    }
}
