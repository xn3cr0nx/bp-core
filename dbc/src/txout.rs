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

use amplify::Wrapper;
use bitcoin::hashes::{sha256, Hmac};
use bitcoin::{secp256k1, TxOut};
use bitcoin_scripts::PubkeyScript;
use commit_verify::EmbedCommitVerify;

use super::{
    Container, Error, Proof, ScriptEncodeData, ScriptEncodeMethod,
    SpkCommitment, SpkContainer,
};

#[derive(Clone, PartialEq, Eq, Debug, Display)]
#[display(Debug)]
pub struct TxoutContainer {
    pub value: u64,
    pub script_container: SpkContainer,
    /// Tweaking factor stored after [`TxoutCommitment::embed_commit`]
    /// procedure
    pub tweaking_factor: Option<Hmac<sha256::Hash>>,
}

impl TxoutContainer {
    pub fn construct(
        protocol_tag: &sha256::Hash,
        value: u64,
        pubkey: secp256k1::PublicKey,
        source: ScriptEncodeData,
        method: ScriptEncodeMethod,
    ) -> Self {
        Self {
            value,
            script_container: SpkContainer::construct(
                protocol_tag,
                pubkey,
                source,
                method,
            ),
            tweaking_factor: None,
        }
    }
}

impl Container for TxoutContainer {
    /// Out supplement is a protocol-specific tag in its hashed form
    type Supplement = sha256::Hash;
    type Host = TxOut;

    fn reconstruct(
        proof: &Proof,
        supplement: &Self::Supplement,
        host: &Self::Host,
    ) -> Result<Self, Error> {
        Ok(Self {
            value: host.value,
            script_container: SpkContainer::reconstruct(
                proof,
                supplement,
                &PubkeyScript::from_inner(host.clone().script_pubkey),
            )?,
            tweaking_factor: None,
        })
    }

    fn deconstruct(self) -> (Proof, Self::Supplement) {
        self.script_container.deconstruct()
    }

    fn to_proof(&self) -> Proof { self.script_container.to_proof() }

    fn into_proof(self) -> Proof { self.script_container.into_proof() }
}

/// [`bitcoin::TxOut`] containing LNPBP-2 commitment
#[derive(Wrapper, Clone, PartialEq, Eq, Hash, Default, Debug, Display, From)]
#[display(Debug)]
pub struct TxoutCommitment(TxOut);

impl<MSG> EmbedCommitVerify<MSG> for TxoutCommitment
where
    MSG: AsRef<[u8]>,
{
    type Container = TxoutContainer;
    type Error = Error;

    fn embed_commit(
        container: &mut Self::Container,
        msg: &MSG,
    ) -> Result<Self, Self::Error> {
        let commitment = TxOut {
            value: container.value,
            script_pubkey: (**SpkCommitment::embed_commit(
                &mut container.script_container,
                msg,
            )?)
            .clone(),
        };

        container.tweaking_factor = container.script_container.tweaking_factor;

        Ok(commitment.into())
    }
}
