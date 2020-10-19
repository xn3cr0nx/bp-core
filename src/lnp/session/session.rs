// LNP/BP Core Library implementing LNPBP specifications & standards
// Written in 2020 by
//     Dr. Maxim Orlovsky <orlovsky@pandoracore.com>
//
// To the extent possible under law, the author(s) have dedicated all
// copyright and related and neighboring rights to this software to
// the public domain worldwide. This software is distributed without
// any warranty.
//
// You should have received a copy of the MIT License
// along with this software.
// If not, see <https://opensource.org/licenses/MIT>.

use amplify::{AsAny, Bipolar};
use core::borrow::Borrow;

use super::{Decrypt, Encrypt, NodeLocator, Transcode};
use crate::lnp::session::NoEncryption;
use crate::lnp::transport::zmqsocket::{
    ApiType as ZmqType, Connection, SocketLocator,
};
use crate::lnp::transport::{
    self, Duplex, Error, Receiver, RecvFrame, SendFrame, Sender,
};

pub trait SessionTrait: Bipolar + AsAny {}

pub struct Session<T, S>
where
    T: Transcode,
    S: Duplex,
{
    transcoder: T,
    stream: S,
}

pub struct Inbound<D, I>
where
    D: Decrypt,
    I: Receiver,
{
    pub(self) decryptor: D,
    pub(self) input: I,
}

pub struct Outbound<E, O>
where
    E: Encrypt,
    O: Sender,
{
    pub(self) encryptor: E,
    pub(self) output: O,
}

impl<T, S> Session<T, S>
where
    T: Transcode,
    S: Duplex,
{
    pub fn new(_node_locator: NodeLocator) -> Result<Self, Error> {
        unimplemented!()
    }
}

impl Session<NoEncryption, transport::zmqsocket::Connection> {
    pub fn new_zmq_unencrypted(
        zmq_type: ZmqType,
        context: &zmq::Context,
        remote: SocketLocator,
        local: Option<SocketLocator>,
    ) -> Result<Self, Error> {
        Ok(Self {
            transcoder: NoEncryption,
            stream: Connection::new(zmq_type, context, remote, local)?,
        })
    }

    pub fn as_socket(&self) -> &zmq::Socket {
        &self.stream.as_socket()
    }
}

impl<T, S> Bipolar for Session<T, S>
where
    T: Transcode,
    T::Left: Decrypt,
    T::Right: Encrypt,
    S: Duplex,
    S::Left: Receiver,
    S::Right: Sender,
{
    type Left = Inbound<T::Left, S::Left>;
    type Right = Outbound<T::Right, S::Right>;

    fn join(_left: Self::Left, _right: Self::Right) -> Self {
        unimplemented!()
    }

    fn split(self) -> (Self::Left, Self::Right) {
        unimplemented!()
    }
}

impl<T, S> Session<T, S>
where
    T: Transcode,
    S: Duplex,
    Error: From<T::Error>,
{
    pub fn recv_raw_message(&mut self) -> Result<Vec<u8>, Error> {
        let reader = self.stream.receiver();
        Ok(self.transcoder.decrypt(reader.recv_frame()?)?)
    }

    pub fn send_raw_message(
        &mut self,
        raw: impl Borrow<[u8]>,
    ) -> Result<usize, Error> {
        let writer = self.stream.sender();
        Ok(writer.send_frame(self.transcoder.encrypt(raw))?)
    }
}
