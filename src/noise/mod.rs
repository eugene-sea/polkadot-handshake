// Copyright 2019 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

mod io;
mod protocol;

use handshake::State;
use io::{handshake, Output};
use libp2p::{
    core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo},
    futures::prelude::*,
    identity::{self, PeerId},
};
use protocol::{noise_params_into_builder, AuthenticKeypair, Keypair, PARAMS_XX};
use snow::params::NoiseParams;
use std::pin::Pin;

/// The configuration for the noise handshake.
#[derive(Clone)]
pub struct Config {
    dh_keys: AuthenticKeypair,
    params: NoiseParams,
}

impl Config {
    /// Construct a new configuration for the noise handshake using the XX handshake pattern.
    pub fn new(identity: &identity::Keypair) -> Result<Self, Error> {
        let noise_keys = Keypair::new().into_authentic(identity)?;

        Ok(Self {
            dh_keys: noise_keys,
            params: PARAMS_XX.clone(),
        })
    }

    fn into_responder<S>(self, socket: S) -> Result<State<S>, Error> {
        let session = noise_params_into_builder(self.params, self.dh_keys.keypair.secret(), None)
            .build_responder()?;

        let state = State::new(socket, session, self.dh_keys.identity, None);

        Ok(state)
    }

    fn into_initiator<S>(self, socket: S) -> Result<State<S>, Error> {
        let session = noise_params_into_builder(self.params, self.dh_keys.keypair.secret(), None)
            .build_initiator()?;

        let state = State::new(socket, session, self.dh_keys.identity, None);

        Ok(state)
    }
}

impl UpgradeInfo for Config {
    type Info = &'static str;
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once("/noise")
    }
}

impl<T> InboundUpgrade<T> for Config
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = (PeerId, Output<T>);
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, socket: T, _: Self::Info) -> Self::Future {
        async move {
            let mut state = self.into_responder(socket)?;

            handshake::recv_empty(&mut state).await?;
            handshake::send_identity(&mut state).await?;
            handshake::recv_identity(&mut state).await?;

            let (pk, io) = state.finish()?;

            Ok((pk.to_peer_id(), io))
        }
        .boxed()
    }
}

impl<T> OutboundUpgrade<T> for Config
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = (PeerId, Output<T>);
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, socket: T, _: Self::Info) -> Self::Future {
        async move {
            let mut state = self.into_initiator(socket)?;

            handshake::send_empty(&mut state).await?;
            handshake::recv_identity(&mut state).await?;
            handshake::send_identity(&mut state).await?;

            let (pk, io) = state.finish()?;

            Ok((pk.to_peer_id(), io))
        }
        .boxed()
    }
}

/// libp2p_noise error type.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Noise(#[from] snow::Error),
    #[error("Invalid public key")]
    InvalidKey(#[from] libp2p::identity::DecodingError),
    #[error("Only keys of length 32 bytes are supported")]
    InvalidLength,
    #[error("The signature of the remote identity's public key does not verify")]
    BadSignature,
    #[error("Authentication failed")]
    AuthenticationFailed,
    #[error("failed to decode protobuf ")]
    InvalidPayload(#[from] DecodeError),
    #[error(transparent)]
    SigningError(#[from] libp2p::identity::SigningError),
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct DecodeError(quick_protobuf::Error);
