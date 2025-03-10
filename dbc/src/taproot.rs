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

use bitcoin::hashes::{sha256, Hmac};
use bitcoin::secp256k1;
use commit_verify::EmbedCommitVerify;

use super::{
    Container, Error, Proof, PubkeyCommitment, PubkeyContainer,
    ScriptEncodeData,
};

#[derive(Clone, PartialEq, Eq, Hash, Debug, Display)]
#[display(Debug)]
pub struct TaprootContainer {
    pub script_root: sha256::Hash,
    pub intermediate_key: secp256k1::PublicKey,
    /// Single SHA256 hash of the protocol-specific tag
    pub tag: sha256::Hash,
    /// Tweaking factor stored after [`TaprootCommitment::embed_commit`]
    /// procedure
    pub tweaking_factor: Option<Hmac<sha256::Hash>>,
}

impl Container for TaprootContainer {
    /// Out supplement is a protocol-specific tag in its hashed form
    type Supplement = sha256::Hash;
    /// Our proof contains the host, so we don't need host here
    type Host = Option<()>;

    fn reconstruct(
        proof: &Proof,
        supplement: &Self::Supplement,
        _: &Self::Host,
    ) -> Result<Self, Error> {
        if let ScriptEncodeData::Taproot(ref tapscript_root) = proof.source {
            Ok(Self {
                script_root: *tapscript_root,
                intermediate_key: proof.pubkey,
                tag: *supplement,
                tweaking_factor: None,
            })
        } else {
            Err(Error::InvalidProofStructure)
        }
    }

    fn deconstruct(self) -> (Proof, Self::Supplement) {
        (
            Proof {
                pubkey: self.intermediate_key,
                source: ScriptEncodeData::Taproot(self.script_root),
            },
            self.tag,
        )
    }

    fn to_proof(&self) -> Proof {
        Proof {
            pubkey: self.intermediate_key,
            source: ScriptEncodeData::Taproot(self.script_root),
        }
    }

    fn into_proof(self) -> Proof {
        Proof {
            pubkey: self.intermediate_key,
            source: ScriptEncodeData::Taproot(self.script_root),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug, Display)]
#[display(Debug)]
pub struct TaprootCommitment {
    pub script_root: sha256::Hash,
    pub intermediate_key_commitment: PubkeyCommitment,
}

impl<MSG> EmbedCommitVerify<MSG> for TaprootCommitment
where
    MSG: AsRef<[u8]>,
{
    type Container = TaprootContainer;
    type Error = Error;

    fn embed_commit(
        container: &mut Self::Container,
        msg: &MSG,
    ) -> Result<Self, Self::Error> {
        let mut pubkey_container = PubkeyContainer {
            pubkey: container.intermediate_key,
            tag: container.tag,
            tweaking_factor: None,
        };

        let cmt = PubkeyCommitment::embed_commit(&mut pubkey_container, msg)?;

        container.tweaking_factor = pubkey_container.tweaking_factor;

        Ok(Self {
            script_root: container.script_root,
            intermediate_key_commitment: cmt,
        })
    }
}
