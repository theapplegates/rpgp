use log::debug;
use zeroize::ZeroizeOnDrop;

use crate::crypto::sym::SymmetricKeyAlgorithm;
use crate::crypto::{checksum, dsa, ecdh, ecdsa, eddsa, rsa, x25519, Decryptor};
use crate::errors::Result;
use crate::types::PkeskBytes;
use crate::types::{EskType, PublicKeyTrait, PublicParams};
use crate::PlainSessionKey;

use super::EcdhPublicParams;

/// The version of the secret key that is actually exposed to users to do crypto operations.
#[allow(clippy::large_enum_variant)] // FIXME
#[derive(Debug, Clone, PartialEq, Eq, ZeroizeOnDrop)]
pub enum SecretKeyRepr {
    RSA(rsa::PrivateKey),
    DSA(dsa::SecretKey),
    ECDSA(ecdsa::SecretKey),
    ECDH(ecdh::SecretKey),
    EdDSA(eddsa::SecretKey),
    EdDSALegacy(eddsa::SecretKey),
    X25519(x25519::SecretKey),
    #[cfg(feature = "unstable-curve448")]
    X448(crate::crypto::x448::SecretKey),
}

impl SecretKeyRepr {
    pub fn decrypt<P>(
        &self,
        pub_params: &PublicParams,
        values: &PkeskBytes,
        typ: EskType,
        recipient: &P,
    ) -> Result<PlainSessionKey>
    where
        P: PublicKeyTrait,
    {
        let decrypted_key = match (self, values) {
            (SecretKeyRepr::RSA(ref priv_key), PkeskBytes::Rsa { mpi }) => priv_key.decrypt(mpi)?,
            (SecretKeyRepr::DSA(_), _) => bail!("DSA is only used for signing"),
            (SecretKeyRepr::ECDSA(_), _) => bail!("ECDSA is only used for signing"),
            (
                SecretKeyRepr::ECDH(ref priv_key),
                PkeskBytes::Ecdh {
                    public_point,
                    encrypted_session_key,
                },
            ) => {
                let PublicParams::ECDH(params) = pub_params else {
                    bail!("inconsistent key state");
                };

                let (hash, alg_sym) = match params {
                    EcdhPublicParams::Curve25519 { hash, alg_sym, .. } => (hash, alg_sym),
                    EcdhPublicParams::P256 { hash, alg_sym, .. } => (hash, alg_sym),
                    EcdhPublicParams::P384 { hash, alg_sym, .. } => (hash, alg_sym),
                    EcdhPublicParams::P521 { hash, alg_sym, .. } => (hash, alg_sym),
                    EcdhPublicParams::Unsupported { curve, .. } => {
                        unsupported_err!("curve {} is not supported", curve);
                    }
                };

                priv_key.decrypt(ecdh::EncryptionFields {
                    public_point,
                    encrypted_session_key,
                    fingerprint: recipient.fingerprint().as_bytes(),
                    curve: params.curve(),
                    hash: *hash,
                    alg_sym: *alg_sym,
                })?
            }

            (
                SecretKeyRepr::X25519(ref priv_key),
                PkeskBytes::X25519 {
                    ephemeral,
                    session_key,
                    sym_alg,
                },
            ) => {
                // Recipient public key (32 bytes)
                let PublicParams::X25519(params) = recipient.public_params() else {
                    bail!(
                        "Unexpected recipient public_params {:?} for X25519",
                        recipient.public_params()
                    );
                };

                let data = x25519::EncryptionFields {
                    ephemeral_public_point: ephemeral.to_owned(),
                    recipient_public: params.key.to_bytes(),
                    encrypted_session_key: session_key,
                };

                let key = priv_key.decrypt(data)?;

                return match (&typ, *sym_alg) {
                    // We expect `sym_alg` to be set for v3 PKESK, and unset for v6 PKESK
                    (EskType::V3_4, Some(sym_alg)) => Ok(PlainSessionKey::V3_4 { key, sym_alg }),
                    (EskType::V6, None) => Ok(PlainSessionKey::V6 { key }),
                    _ => bail!("unexpected: sym_alg {:?} for {:?}", sym_alg, typ),
                };
            }

            #[cfg(feature = "unstable-curve448")]
            (
                SecretKeyRepr::X448(ref priv_key),
                PkeskBytes::X448 {
                    ephemeral,
                    session_key,
                    sym_alg,
                },
            ) => {
                // Recipient public key (56 bytes)
                let PublicParams::X448 { public } = recipient.public_params() else {
                    bail!(
                        "Unexpected recipient public_params {:?} for X448",
                        recipient.public_params()
                    );
                };

                let data = crate::crypto::x448::EncryptionFields {
                    ephemeral_public_point: ephemeral.to_owned(),
                    recipient_public: *public,
                    encrypted_session_key: session_key,
                };

                let key = priv_key.decrypt(data)?;

                // We expect `algo` to be set for v3 PKESK, and unset for v6 PKESK
                return if let Some(sym_alg) = *sym_alg {
                    Ok(PlainSessionKey::V3_4 { key, sym_alg })
                } else {
                    Ok(PlainSessionKey::V6 { key })
                };
            }

            (SecretKeyRepr::EdDSA(_), _) => bail!("EdDSA is only used for signing"),
            _ => unimplemented_err!(
                "Unsupported: SecretKeyRepr {:?}, PkeskBytes {:?}",
                self,
                values
            ),
        };

        match typ {
            EskType::V3_4 => {
                let sym_alg = SymmetricKeyAlgorithm::from(decrypted_key[0]);
                ensure!(
                    sym_alg != SymmetricKeyAlgorithm::Plaintext,
                    "session key algorithm cannot be plaintext"
                );

                debug!("sym_alg: {:?}", sym_alg);

                let key_size = sym_alg.key_size();
                ensure_eq!(
                    decrypted_key.len(),
                    key_size + 3,
                    "unexpected decrypted_key length ({}) for sym_alg {:?}",
                    decrypted_key.len(),
                    sym_alg
                );

                let key = decrypted_key[1..=key_size].to_vec();
                let checksum = &decrypted_key[key_size + 1..key_size + 3];

                checksum::simple(checksum, &key)?;

                Ok(PlainSessionKey::V3_4 { key, sym_alg })
            }

            EskType::V6 => {
                let len = decrypted_key.len();

                // ensure minimal length so that we don't panic in the slice splitting, below
                ensure!(
                    len >= 2,
                    "unexpected decrypted_key length ({}) for V6 ESK",
                    len,
                );

                let key = decrypted_key[0..len - 2].to_vec();
                let checksum = &decrypted_key[len - 2..];

                checksum::simple(checksum, &key)?;

                Ok(PlainSessionKey::V6 { key })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use proptest::prelude::*;

    use crate::crypto::public_key::PublicKeyAlgorithm;

    impl Arbitrary for SecretKeyRepr {
        type Parameters = PublicKeyAlgorithm;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary() -> Self::Strategy {
            any::<PublicKeyAlgorithm>()
                .prop_flat_map(Self::arbitrary_with)
                .boxed()
        }

        fn arbitrary_with(alg: Self::Parameters) -> Self::Strategy {
            match alg {
                PublicKeyAlgorithm::RSA
                | PublicKeyAlgorithm::RSAEncrypt
                | PublicKeyAlgorithm::RSASign => any::<rsa::PrivateKey>()
                    .prop_map(SecretKeyRepr::RSA)
                    .boxed(),
                PublicKeyAlgorithm::DSA => {
                    any::<dsa::SecretKey>().prop_map(SecretKeyRepr::DSA).boxed()
                }
                PublicKeyAlgorithm::ECDSA => any::<ecdsa::SecretKey>()
                    .prop_map(SecretKeyRepr::ECDSA)
                    .boxed(),
                PublicKeyAlgorithm::ECDH => any::<ecdh::SecretKey>()
                    .prop_map(SecretKeyRepr::ECDH)
                    .boxed(),
                PublicKeyAlgorithm::EdDSALegacy => any::<eddsa::SecretKey>()
                    .prop_map(SecretKeyRepr::EdDSALegacy)
                    .boxed(),
                PublicKeyAlgorithm::Ed25519 => any::<eddsa::SecretKey>()
                    .prop_map(SecretKeyRepr::EdDSA)
                    .boxed(),
                PublicKeyAlgorithm::X25519 => any::<x25519::SecretKey>()
                    .prop_map(SecretKeyRepr::X25519)
                    .boxed(),
                #[cfg(feature = "unstable-curve448")]
                PublicKeyAlgorithm::X448 => any::<crate::crypto::x448::SecretKey>()
                    .prop_map(SecretKeyRepr::X448)
                    .boxed(),
                _ => {
                    unimplemented!("{:?}", alg)
                }
            }
        }
    }
}
