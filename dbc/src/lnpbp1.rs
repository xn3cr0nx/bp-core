// BP Core Library implementing LNP/BP specifications & standards related to
// bitcoin protocol
//
// Written in 2020-2021 by
//     Dr. Maxim Orlovsky <orlovsky@pandoracore.com>
//
// To the extent possible under law, the author(s) have dedicated all
// copyright and related and neighboring rights to this software to
// the public domain worldwide. This software is distributed without
// any warranty.
//
// You should have received a copy of the Apache 2.0 License
// along with this software.
// If not, see <https://opensource.org/licenses/Apache-2.0>.

use std::collections::BTreeSet;

use bitcoin::hashes::{sha256, Hash, HashEngine, Hmac, HmacEngine};
use bitcoin::secp256k1;

/// Single SHA256 hash of "LNPBP1" string according to LNPBP-1 acting as a
/// prefix to the message in computing tweaking factor
pub static LNPBP1_HASHED_TAG: [u8; 32] = [
    245, 8, 242, 142, 252, 192, 113, 82, 108, 168, 134, 200, 224, 124, 105,
    212, 149, 78, 46, 201, 252, 82, 171, 140, 204, 209, 41, 17, 12, 0, 64, 175,
];

/// Deterministically-organized set of all public keys used by this mod
/// internally
type Keyset = BTreeSet<secp256k1::PublicKey>;

/// Errors that may happen during LNPBP-1 commitment procedure or because of
/// incorrect arguments provided to [`commit()`] function.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Display, Error, From)]
#[display(doc_comments)]
pub enum Error {
    /// Keyset must include target public key, but no target key found it
    /// the provided set.
    NotKeysetMember,

    /// Elliptic curve point addition resulted in point in infinity; you
    /// must select different source public keys
    SumInfiniteResult,

    /// LNPBP-1 commitment either is outside of Secp256k1 order `n` (this event
    /// has negligible probability <~2^-64), or, when added to the provided
    /// keyset, results in point at infinity. You may try with a different
    /// source message or public keys.
    InvalidTweak,
}

/// Function performs commitment procedure according to LNPBP-1.
///
/// # Parameters
///
/// - A set of public keys for committing during the LNPBP-1 procedure
/// - Target public key for tweaking. Must be a part of the keyset, otherwise
///   function will fail with [`Error::NotKeysetMember`]
/// - Protocol-specific tag in form of 32-byte hash
/// - Message to commit to, which must be representable as a byte slice using
///   [`AsRef::as_ref()`]
///
/// # Returns
///
/// Function mutates two of its parameters,
/// - `target_pubkey`, with a tweaked version of the public key containing
///   commitment to the message and the rest of keyset,
/// - `keyset`, by replacing original `target_pubkey` with its tweaked version
/// and returns `tweaking_factor` as a return parameter wrapped into
/// [`Result::Ok`].
///
/// If the function fails with any error, value for `target_pubkey` and `keyset`
/// is undefined and must be discarded.
///
/// # Errors
///
/// Function may fail because of one of the following circumstances:
/// - If `target_pubkey` is not a part of `keyset` ([`Error::NotKeysetMember`])
/// - If keyset deliberately constructed in a way that sum of some of its keys
///   is equivalent to negation of some other keys. In this case function fails
///   with [`Error::SumInfiniteResult`]
/// - With negligible probability because of elliptic curve Secp256k1 point
///   addition overflow; in this case function returns either
///   [`Error::SumInfiniteResult`], if it happens during summation of public
///   keys from the `keyset`, or [`Error::InvalidTweak`], if it happens during
///   tweaking factor addition to the `target_pubkey`.
///
/// # Protocol:
///
/// Please refer to the original document for the verification:
/// <https://github.com/LNP-BP/LNPBPs/blob/master/lnpbp-0001.md>

// #[consensus_critical("RGB")]
// #[standard_critical("LNPBP-1")]
pub fn commit(
    keyset: &mut Keyset,
    target_pubkey: &mut secp256k1::PublicKey,
    protocol_tag: &sha256::Hash,
    message: &impl AsRef<[u8]>,
) -> Result<Hmac<sha256::Hash>, Error> {
    if !keyset.remove(target_pubkey) {
        return Err(Error::NotKeysetMember);
    }

    // ! [CONSENSUS-CRITICAL]:
    // ! [STANDARD-CRITICAL]: We commit to the sum of all public keys,
    //                        not a single pubkey. For single key the set
    //                        is represented by itself
    let pubkey_sum = keyset
        .iter()
        .try_fold(*target_pubkey, |sum, pubkey| sum.combine(pubkey))
        .map_err(|_| Error::SumInfiniteResult)?;

    // ! [CONSENSUS-CRITICAL]:
    // ! [STANDARD-CRITICAL]: HMAC engine is based on sha256 hash
    let mut hmac_engine =
        HmacEngine::<sha256::Hash>::new(&pubkey_sum.serialize());

    // ! [CONSENSUS-CRITICAL]:
    // ! [STANDARD-CRITICAL]: Hash process started with consuming first
    //                        protocol prefix: single SHA256 hash of
    //                        ASCII "LNPBP1" string.
    // NB: We use the same hash as in LNPBP-1 so when there is no other
    //     keys involved the commitment would not differ.
    hmac_engine.input(&LNPBP1_HASHED_TAG[..]);

    // ! [CONSENSUS-CRITICAL]:
    // ! [STANDARD-CRITICAL]: The second prefix comes from the upstream
    //                        protocol as a part of the container
    hmac_engine.input(&protocol_tag[..]);

    // ! [CONSENSUS-CRITICAL]:
    // ! [STANDARD-CRITICAL]: Next we hash the message. The message must be
    //                        prefixed with the protocol-specific prefix:
    //                        another single SHA256 hash of protocol name.
    //                        However this is not the part of this function,
    //                        the function expect that the `msg` is already
    //                        properly prefixed
    hmac_engine.input(&sha256::Hash::hash(message.as_ref()));

    // Producing tweaking factor
    let tweaking_factor = Hmac::from_engine(hmac_engine);

    // Applying tweaking factor to public key
    target_pubkey
        .add_exp_assign(secp256k1::SECP256K1, &tweaking_factor[..])
        .map_err(|_| Error::InvalidTweak)?;

    keyset.insert(*target_pubkey);

    // Returning tweaked public key
    Ok(tweaking_factor)
}

/// Function verifies commitment created according to LNPBP-1.
///
/// # Parameters
///
/// - `verified_pubkey`: public key containing LNPBP-1 commitment, i.e. the one
///   modified by [`commit()`] procedure as its second parameter `target_key`
/// - `original_keyset`: set of public keys provided to the [`commit()`]
///   procedure. This set must include orignal pubkey specified in the next
///   parameter `taget_pubkey`
/// - `target_pubkey`: one of public keys included into the original keyset and
///   that was provided to the [`commit()`] procedure as `target_pubkey`. This
///   must be an original version of public key from the `verified_pubkey`
///   parameter before the tweak was applied
/// - `protocol_tag`: protocol-specific tag in form of 32-byte hash
/// - `message`: message to commit to, which must be representable as a byte
///   slice using [`AsRef::as_ref()`]
///
/// # Returns
///
/// - `true`, if verification succeeds,
/// - `false`, if verification fails, indicating that the provided
///   `verified_pubkey` is not committed to the data given in the rest of
///   function parameters.
///
/// # Procedure
///
/// Please refer to the original document for the general algotirhm:
/// <https://github.com/LNP-BP/LNPBPs/blob/master/lnpbp-0001.md>
///
/// Function verifies commitment by running LNPBP-1 commitment procedure once
/// again with the provided data as a source data, and comparing the result of
/// the commitment to the `verified_pubkey`. If the commitment function fails,
/// it means that it was not able to commit with the provided data, meaning that
/// the commitment was not created. Thus, we return that verification have not
/// passed, and not a error. Verification succeeds if the commitment procedure
/// produces public key equivalent to the `verified_pubkey`.
pub fn verify(
    verified_pubkey: secp256k1::PublicKey,
    original_keyset: &Keyset,
    mut target_pubkey: secp256k1::PublicKey,
    protocol_tag: &sha256::Hash,
    message: &impl AsRef<[u8]>,
) -> bool {
    match commit(
        &mut original_keyset.clone(),
        &mut target_pubkey,
        protocol_tag,
        message,
    ) {
        // If the commitment function fails, it means that it was not able to
        // commit with the provided data, meaning that the commitment was not
        // created. Thus, we return that verification have not passed, and not
        // a error.
        Err(_) => false,

        // Verification succeeds if the commitment procedure produces public key
        // equivalent to the verified one
        Ok(_) => target_pubkey == verified_pubkey,
    }
}

/// Helpers for writing test functions working with commit-verify scheme
#[cfg(test)]
pub mod test_helpers {
    use std::collections::HashSet;
    use std::fmt::Debug;

    use amplify::hex::FromHex;
    use commit_verify::EmbedCommitVerify;

    use super::*;

    /// Generates a set of messages for testing purposes
    ///
    /// All of these messages MUST produce different commitments, otherwise the
    /// commitment algorithm is not collision-resistant
    pub fn gen_messages() -> Vec<Vec<u8>> {
        vec![
            // empty message
            b"".to_vec(),
            // zero byte message
            b"\x00".to_vec(),
            // text message
            b"test".to_vec(),
            // text length-extended message
            b"test*".to_vec(),
            // short binary message
            Vec::from_hex("deadbeef").unwrap(),
            // length-extended version
            Vec::from_hex("deadbeef00").unwrap(),
            // prefixed version
            Vec::from_hex("00deadbeef").unwrap(),
            // serialized public key as text
            b"0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798".to_vec(),
            // the same public key binary data
            Vec::from_hex("0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
                .unwrap(),
            // different public key
            Vec::from_hex("02f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9")
                .unwrap(),
        ]
    }

    pub fn gen_secp_pubkeys(n: usize) -> Vec<secp256k1::PublicKey> {
        let mut ret = Vec::with_capacity(n);
        let mut sk = [0; 32];

        for i in 1..n + 1 {
            sk[0] = i as u8;
            sk[1] = (i >> 8) as u8;
            sk[2] = (i >> 16) as u8;

            ret.push(secp256k1::PublicKey::from_secret_key(
                &secp256k1::SECP256K1,
                &secp256k1::SecretKey::from_slice(&sk[..]).unwrap(),
            ));
        }
        ret
    }

    /// Runs round-trip of commitment-embed-verify for a given set of messages
    /// and provided container
    pub fn embed_commit_verify_suite<MSG, CMT>(
        messages: Vec<MSG>,
        container: &mut CMT::Container,
    ) where
        MSG: AsRef<[u8]> + Eq,
        CMT: EmbedCommitVerify<MSG> + Eq + std::hash::Hash + Debug,
    {
        messages.iter().fold(
            HashSet::<CMT>::with_capacity(messages.len()),
            |mut acc, msg| {
                let commitment = CMT::embed_commit(container, msg).unwrap();

                // Commitments MUST be deterministic: each message should
                // produce unique commitment
                (1..10).for_each(|_| {
                    assert_eq!(
                        CMT::embed_commit(container, msg).unwrap(),
                        commitment
                    );
                });

                // Testing verification
                assert!(commitment.verify(container, msg).unwrap());

                messages.iter().for_each(|m| {
                    // Testing that commitment verification succeeds only
                    // for the original message and fails for the rest
                    assert_eq!(
                        commitment.verify(container, m).unwrap(),
                        m == msg
                    );
                });

                acc.iter().for_each(|cmt| {
                    // Testing that verification against other commitments
                    // returns `false`
                    assert!(!cmt.verify(container, msg).unwrap());
                });

                // Detecting collision
                assert!(acc.insert(commitment));

                acc
            },
        );
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use super::*;
    use crate::lnpbp1::test_helpers::*;

    #[test]
    fn test_lnpbp1_tag() {
        assert_eq!(
            sha256::Hash::hash(b"LNPBP1").into_inner(),
            LNPBP1_HASHED_TAG
        );
        assert_ne!(
            sha256::Hash::hash(b"LNPBP2").into_inner(),
            LNPBP1_HASHED_TAG
        );
        assert_ne!(
            sha256::Hash::hash(b"LNPBP-1").into_inner(),
            LNPBP1_HASHED_TAG
        );
        assert_ne!(
            sha256::Hash::hash(b"LNPBP_1").into_inner(),
            LNPBP1_HASHED_TAG
        );
        assert_ne!(
            sha256::Hash::hash(b"lnpbp1").into_inner(),
            LNPBP1_HASHED_TAG
        );
        assert_ne!(
            sha256::Hash::hash(b"lnpbp-1").into_inner(),
            LNPBP1_HASHED_TAG
        );
        assert_ne!(
            sha256::Hash::hash(b"lnpbp_1").into_inner(),
            LNPBP1_HASHED_TAG
        );
    }

    #[test]
    fn test_single_key() {
        let tag = sha256::Hash::hash(b"ProtoTag");
        let tag2 = sha256::Hash::hash(b"Prototag");
        let messages = gen_messages();
        let all_keys = gen_secp_pubkeys(6);
        let other_key = all_keys[0];
        for msg in &messages {
            for mut pk in all_keys[1..].to_vec() {
                let original = pk.clone();
                let mut keyset = bset![pk];
                let mut keyset2 = bset![pk];
                let mut pk2 = pk.clone();
                let factor1 = commit(&mut keyset, &mut pk, &tag, &msg).unwrap();
                let factor2 =
                    commit(&mut keyset2, &mut pk2, &tag2, &msg).unwrap();

                // Ensure that changing tag changes commitment and tweaking
                // factor (and tag is case-sensitive!)
                assert_ne!(factor1, factor2);
                assert_ne!(pk, pk2);

                // Ensure that factor value is not trivial
                assert_ne!(factor1, Hmac::from_slice(&[0u8; 32]).unwrap());
                assert_ne!(factor1, Hmac::from_slice(&[1u8; 32]).unwrap());
                assert_ne!(factor1, Hmac::from_slice(&[0xFFu8; 32]).unwrap());
                assert_ne!(&factor1[..], &tag[..]);
                assert_ne!(&factor1[..], &msg[..]);

                // Verify that the key was indeed tweaked
                assert_ne!(pk, original);

                // Verify that the set updated
                assert_ne!(bset![original], keyset);
                assert_eq!(bset![pk], keyset);

                // Do commitment by hand
                let mut engine =
                    HmacEngine::<sha256::Hash>::new(&original.serialize());
                engine.input(&LNPBP1_HASHED_TAG);
                engine.input(&tag.into_inner());
                engine.input(&sha256::Hash::hash(msg));
                let hmac = Hmac::from_engine(engine);
                let tweaking_factor = *hmac.as_inner();
                let mut altkey = original;
                altkey
                    .add_exp_assign(&secp256k1::SECP256K1, &tweaking_factor[..])
                    .unwrap();
                assert_eq!(altkey, pk);

                // Now try commitment with a different key, but the same data
                if other_key != original {
                    let mut other_commitment = other_key;
                    let mut other_keyset = bset![other_commitment];
                    let factor3 = commit(
                        &mut other_keyset,
                        &mut other_commitment,
                        &tag,
                        &msg,
                    )
                    .unwrap();

                    // Make sure we commit to the key value
                    assert_ne!(factor1, factor3);

                    // Make sure commitment value is not the same
                    assert_ne!(pk, other_commitment);

                    // Make sure we can't cross-verify
                    assert_eq!(
                        verify(
                            other_commitment,
                            &bset![original],
                            original,
                            &tag,
                            &msg
                        ),
                        false
                    );
                }

                // Verify commitment
                assert!(verify(pk, &bset![original], original, &tag, &msg));

                // Make sure we can't cross-verify with different tag
                assert_eq!(
                    verify(pk, &bset![original], original, &tag2, &msg),
                    false
                );

                // Make sure we can't cross-verify with different message
                assert_eq!(
                    verify(
                        pk,
                        &bset![original],
                        original,
                        &tag2,
                        &b"some other message"
                    ),
                    false
                );
            }
        }
    }

    #[test]
    fn test_keyset() {
        let tag = sha256::Hash::hash(b"ProtoTag");
        let tag2 = sha256::Hash::hash(b"Prototag");
        let messages = gen_messages();
        let all_keys = gen_secp_pubkeys(6);
        let other_key = all_keys[0];
        let original_keyset: BTreeSet<_> =
            all_keys[1..].to_vec().into_iter().collect();
        for msg in &messages {
            for mut pk in original_keyset.clone() {
                let original = pk.clone();
                let mut keyset = original_keyset.clone();
                let mut keyset2 = original_keyset.clone();
                let mut pk2 = pk.clone();
                let factor1 = commit(&mut keyset, &mut pk, &tag, &msg).unwrap();
                let factor2 =
                    commit(&mut keyset2, &mut pk2, &tag2, &msg).unwrap();

                // Ensure that changing tag changes commitment and tweaking
                // factor (and tag is case-sensitive!)
                assert_ne!(factor1, factor2);
                assert_ne!(pk, pk2);

                // Ensure that factor value is not trivial
                assert_ne!(factor1, Hmac::from_slice(&[0u8; 32]).unwrap());
                assert_ne!(factor1, Hmac::from_slice(&[1u8; 32]).unwrap());
                assert_ne!(factor1, Hmac::from_slice(&[0xFFu8; 32]).unwrap());
                assert_ne!(&factor1[..], &tag[..]);
                assert_ne!(&factor1[..], &msg[..]);

                // Verify that the key was indeed tweaked
                assert_ne!(pk, original);

                // Verify that the set updated
                assert_ne!(original_keyset.clone(), keyset);
                // ... but only original key is touched
                let mut set = keyset.clone();
                set.remove(&pk);
                set.insert(original);
                assert_eq!(set, original_keyset);

                // Do commitment by hand
                let mut engine =
                    HmacEngine::<sha256::Hash>::new(&original.serialize());
                engine.input(&LNPBP1_HASHED_TAG);
                engine.input(&tag.into_inner());
                engine.input(msg);
                let hmac = Hmac::from_engine(engine);
                let tweaking_factor = *hmac.as_inner();
                let mut altkey = original;
                altkey
                    .add_exp_assign(&secp256k1::SECP256K1, &tweaking_factor[..])
                    .unwrap();
                // It must not match because done with a single key, not
                // their sum
                assert_ne!(altkey, pk);

                // Now try commitment with a different key, but the same
                // data
                if other_key != original {
                    let mut other_pk = other_key;
                    let mut other_keyset = original_keyset.clone();
                    assert!(!other_keyset.contains(&other_pk));
                    other_keyset.remove(&pk);
                    other_keyset.insert(other_pk);
                    let factor3 =
                        commit(&mut other_keyset, &mut other_pk, &tag, &msg)
                            .unwrap();

                    // Make sure we commit to the key value
                    assert_ne!(factor1, factor3);

                    // Make sure commitment value is not the same
                    assert_ne!(pk, other_pk);

                    // Make sure we can't cross-verify
                    assert_eq!(
                        verify(
                            other_pk,
                            &bset![original],
                            original,
                            &tag,
                            &msg
                        ),
                        false
                    );
                    assert_eq!(
                        verify(
                            other_pk,
                            &original_keyset,
                            original,
                            &tag,
                            &msg
                        ),
                        false
                    );
                }

                // Verify commitment
                assert!(verify(pk, &original_keyset, original, &tag, &msg));

                // Make sure we can't cross-verify with a single key in a set
                assert_eq!(
                    verify(pk, &bset![original], original, &tag, &msg),
                    false
                );

                // Make sure we can't cross-verify with different tag
                assert_eq!(
                    verify(pk, &original_keyset, original, &tag2, &msg),
                    false
                );

                // Make sure we can't cross-verify with different message
                assert_eq!(
                    verify(
                        pk,
                        &original_keyset,
                        original,
                        &tag2,
                        &b"some other message"
                    ),
                    false
                );
            }
        }
    }

    #[test]
    #[should_panic(expected = "NotKeysetMember")]
    fn test_failure_not_in_keyset() {
        let tag = sha256::Hash::hash(b"ProtoTag");
        let all_keys = gen_secp_pubkeys(6);
        let mut pk = all_keys[0];
        let mut keyset: BTreeSet<_> =
            all_keys[1..].to_vec().into_iter().collect();
        let _ = commit(&mut keyset, &mut pk, &tag, b"Message").unwrap();
    }

    #[test]
    #[should_panic(expected = "SumInfiniteResult")]
    fn test_crafted_negation() {
        let tag = sha256::Hash::hash(b"ProtoTag");
        let mut pubkey = secp256k1::PublicKey::from_str(
            "0218845781f631c48f1c9709e23092067d06837f30aa0cd0544ac887fe91ddd166",
        )
            .unwrap();
        let negkey = secp256k1::PublicKey::from_str(
            "0318845781f631c48f1c9709e23092067d06837f30aa0cd0544ac887fe91ddd166",
        )
            .unwrap();
        let mut keyset = bset![pubkey, negkey];
        let _ = commit(&mut keyset, &mut pubkey, &tag, b"Message").unwrap();
    }
}
