use pgp::crypto::{hash::HashAlgorithm, sym::SymmetricKeyAlgorithm};
use pgp::types::CompressionAlgorithm;
use pgp::{KeyType, SecretKey, SecretKeyParamsBuilder, SubkeyParamsBuilder};
use rand::thread_rng;
use smallvec::smallvec;

pub mod key;
pub mod message;
pub mod s2k;

pub fn build_key(kt: KeyType, kt_sub: KeyType) -> SecretKey {
    let key_params = SecretKeyParamsBuilder::default()
        .key_type(kt)
        .can_certify(true)
        .can_sign(true)
        .primary_user_id("Me <me@mail.com>".into())
        .preferred_symmetric_algorithms(smallvec![
            SymmetricKeyAlgorithm::AES256,
            SymmetricKeyAlgorithm::AES192,
            SymmetricKeyAlgorithm::AES128,
        ])
        .preferred_hash_algorithms(smallvec![
            HashAlgorithm::SHA2_256,
            HashAlgorithm::SHA2_384,
            HashAlgorithm::SHA2_512,
            HashAlgorithm::SHA2_224,
            HashAlgorithm::SHA1,
        ])
        .preferred_compression_algorithms(smallvec![
            CompressionAlgorithm::ZLIB,
            CompressionAlgorithm::ZIP,
        ])
        .passphrase(None)
        .subkey(
            SubkeyParamsBuilder::default()
                .key_type(kt_sub)
                .passphrase(None)
                .can_encrypt(true)
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();
    key_params
        .generate(thread_rng())
        .expect("failed to generate secret key, encrypted")
}
