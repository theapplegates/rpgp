use log::debug;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::crypto::sym::SymmetricKeyAlgorithm;
use crate::errors::Result;
use crate::packet::SymKeyEncryptedSessionKey;
use crate::types::{EskType, Password, PkeskBytes, SkeskVersion};
use crate::SignedSecretKey;

/// Decrypts session key using secret key.
pub fn decrypt_session_key(
    locked_key: &SignedSecretKey,
    key_pw: &Password,
    values: &PkeskBytes,
    typ: EskType,
) -> Result<PlainSessionKey> {
    debug!("decrypt session key");

    locked_key.unlock(key_pw, |pub_params, priv_key| {
        priv_key.decrypt(pub_params, values, typ, &*locked_key.public_key())
    })
}

/// Decrypted session key.
///
/// A v3/v4 session key can be used  v1 SEIPD (and historically with SED packets).
/// A v6 session key can only be used with a v2 SEIPD.
///
/// <https://www.rfc-editor.org/rfc/rfc9580.html#name-packet-versions-in-encrypte>
///
/// (Note that SED packets are malleable. They are historical and considered dangerous!
/// They MUST NOT be produced and decryption is also discouraged:
/// <https://www.rfc-editor.org/rfc/rfc9580.html#sed>)
#[derive(derive_more::Debug, Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub enum PlainSessionKey {
    /// A session key from a v3 PKESK or a v4 SKESK
    ///
    /// (Note: for historical reasons, the OpenPGP format doesn't specify a v4 PKESK or a V3 SKESK)
    V3_4 {
        sym_alg: SymmetricKeyAlgorithm,
        #[debug("..")]
        key: Vec<u8>,
    },

    V5 {
        #[debug("..")]
        key: Vec<u8>,
    },

    /// A session key from a v6 PKESK or a v6 SKESK
    V6 {
        #[debug("..")]
        key: Vec<u8>,
    },
}

/// Decrypts session key from SKESK packet.
///
/// Returns decrypted or derived session key
/// and symmetric algorithm of the key.
pub fn decrypt_session_key_with_password(
    packet: &SymKeyEncryptedSessionKey,
    msg_pw: &Password,
) -> Result<PlainSessionKey> {
    debug!("decrypt session key with password");

    let packet_algorithm = packet.sym_algorithm();
    ensure!(
        packet_algorithm != SymmetricKeyAlgorithm::Plaintext,
        "SKESK packet encryption algorithm cannot be plaintext"
    );

    // Implementations MUST NOT decrypt a secret using MD5, SHA-1, or RIPEMD-160 as a hash function
    // in an S2K KDF in a version 6 (or later) packet.
    //
    // (See https://www.rfc-editor.org/rfc/rfc9580.html#section-9.5-3)
    if packet.version() == SkeskVersion::V6 {
        ensure!(
            !packet.s2k().known_weak_hash_algo(),
            "Weak hash algorithm in S2K not allowed for v6 {:?}",
            packet.s2k()
        )
    }

    let key = packet
        .s2k()
        .derive_key(&msg_pw.read(), packet_algorithm.key_size())?;

    debug!("derived key: {}", hex::encode(&key));
    if packet.encrypted_key().is_empty() {
        // There is no encrypted session key.
        //
        // S2K-derived key is the session key.
        return Ok(PlainSessionKey::V3_4 {
            key,
            sym_alg: packet_algorithm,
        });
    }

    let decrypted_key = packet.decrypt(&key)?;

    Ok(decrypted_key)
}
