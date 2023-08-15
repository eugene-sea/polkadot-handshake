use std::{
    num::NonZeroUsize,
    task::{Context, Poll},
    time::Duration,
};

use libp2p::{
    core::{muxing::StreamMuxerBox, transport::Boxed, upgrade, Endpoint},
    futures::StreamExt,
    identify,
    identity::{self, PublicKey},
    noise,
    swarm::{
        ConnectionClosed, ConnectionDenied, ConnectionId, DialFailure, FromSwarm, ListenFailure,
        NetworkBehaviour, PollParameters, SwarmBuilder, THandler, THandlerInEvent,
        THandlerOutEvent, ToSwarm,
    },
    tcp, Multiaddr, PeerId, Transport,
};
use tracing::{error, info};

const YAMUX_MAXIMUM_BUFFER_SIZE: usize = 16 * 1024 * 1024;
const NODE_NAME: &str = "test-node";
const CLIENT_VERSION: &str = "test-client";
const DIALING_PEER_ID: &str = "12D3KooWBzri3QhYjStNCTvhwrmLExYZjy29hfCG4tbjwixKfHCt";

#[tokio::main]
async fn main() {
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(tracing::Level::DEBUG)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("Can set default subscriber");

    // Generate identity keys
    let local_identity = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_identity.public());

    info!(id = local_peer_id.to_base58(), "Local node identity");

    // Create TCP Transport
    let transport = build_transport(local_identity.clone(), YAMUX_MAXIMUM_BUFFER_SIZE);

    let user_agent = format!("{} ({})", CLIENT_VERSION, NODE_NAME);
    let behaviour = PeerHandshakeBehaviour::new(user_agent, local_identity.public());

    // Create swarm
    let mut swarm = SwarmBuilder::with_tokio_executor(transport, behaviour, local_peer_id)
        .substream_upgrade_protocol_override(upgrade::Version::V1Lazy)
        .notify_handler_buffer_size(NonZeroUsize::new(32).expect("32 != 0"))
        .per_connection_event_buffer_size(24)
        .max_negotiating_inbound_streams(2048)
        .build();

    // Dial the peer
    let dialing_peer_id: PeerId = DIALING_PEER_ID.parse().expect("Can parse dialing peer ID");
    swarm
        .dial(
            format!("/ip4/127.0.0.1/tcp/30333/p2p/{}", dialing_peer_id)
                .parse::<Multiaddr>()
                .expect("Can parse `Multiaddr`"),
        )
        .expect("Can dial peer");

    loop {
        let event = swarm.select_next_some().await;
        info!(?event);
    }
}

fn build_transport(
    keypair: identity::Keypair,
    yamux_maximum_buffer_size: usize,
) -> Boxed<(PeerId, StreamMuxerBox)> {
    let tcp_config = tcp::Config::new().nodelay(true);
    let authentication_config = noise::Config::new(&keypair).expect("Can create noise config");
    let multiplexing_config = {
        let mut yamux_config = libp2p::yamux::Config::default();
        // Enable proper flow-control: window updates are only sent when
        // buffered data has been consumed.
        yamux_config.set_window_update_mode(libp2p::yamux::WindowUpdateMode::on_read());
        yamux_config.set_max_buffer_size(yamux_maximum_buffer_size);

        yamux_config
    };

    tcp::tokio::Transport::new(tcp_config)
        .upgrade(upgrade::Version::V1Lazy)
        .authenticate(authentication_config)
        .multiplex(multiplexing_config)
        .timeout(Duration::from_secs(20))
        .boxed()
}

struct PeerHandshakeBehaviour {
    /// Periodically identifies the remote and responds to incoming requests.
    identify: identify::Behaviour,
}

impl PeerHandshakeBehaviour {
    fn new(user_agent: String, local_public_key: PublicKey) -> Self {
        let identify = {
            let cfg = identify::Config::new("/substrate/1.0".to_string(), local_public_key)
                .with_agent_version(user_agent)
                // We don't need any peer information cached.
                .with_cache_size(0);
            identify::Behaviour::new(cfg)
        };

        Self { identify }
    }
}

/// Event that can be emitted by the behaviour.
#[derive(Debug)]
pub enum PeerInfoEvent {
    /// We have obtained identity information from a peer, including the addresses it is listening
    /// on.
    Identified {
        /// Id of the peer that has been identified.
        peer_id: PeerId,
        /// Information about the peer.
        info: identify::Info,
    },
}

impl NetworkBehaviour for PeerHandshakeBehaviour {
    type ConnectionHandler = <identify::Behaviour as NetworkBehaviour>::ConnectionHandler;

    type ToSwarm = PeerInfoEvent;

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.identify
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _maybe_peer: Option<PeerId>,
        _addresses: &[Multiaddr],
        _effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        // Only `Discovery::handle_pending_outbound_connection` must be returning addresses to
        // ensure that we don't return unwanted addresses.
        Ok(Vec::new())
    }

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        let identify_handler = self.identify.handle_established_inbound_connection(
            connection_id,
            peer,
            local_addr,
            remote_addr,
        )?;
        Ok(identify_handler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        let identify_handler = self.identify.handle_established_outbound_connection(
            connection_id,
            peer,
            addr,
            role_override,
        )?;
        Ok(identify_handler)
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        match event {
            FromSwarm::ConnectionEstablished(e) => {
                self.identify
                    .on_swarm_event(FromSwarm::ConnectionEstablished(e));
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                endpoint,
                handler,
                remaining_established,
            }) => {
                self.identify
                    .on_swarm_event(FromSwarm::ConnectionClosed(ConnectionClosed {
                        peer_id,
                        connection_id,
                        endpoint,
                        handler,
                        remaining_established,
                    }));
            }
            FromSwarm::DialFailure(DialFailure {
                peer_id,
                error,
                connection_id,
            }) => {
                self.identify
                    .on_swarm_event(FromSwarm::DialFailure(DialFailure {
                        peer_id,
                        error,
                        connection_id,
                    }));
            }
            FromSwarm::ListenerClosed(e) => {
                self.identify.on_swarm_event(FromSwarm::ListenerClosed(e));
            }
            FromSwarm::ListenFailure(ListenFailure {
                local_addr,
                send_back_addr,
                error,
                connection_id,
            }) => {
                self.identify
                    .on_swarm_event(FromSwarm::ListenFailure(ListenFailure {
                        local_addr,
                        send_back_addr,
                        error,
                        connection_id,
                    }));
            }
            FromSwarm::ListenerError(e) => {
                self.identify.on_swarm_event(FromSwarm::ListenerError(e));
            }
            FromSwarm::ExternalAddrExpired(e) => {
                self.identify
                    .on_swarm_event(FromSwarm::ExternalAddrExpired(e));
            }
            FromSwarm::NewListener(e) => {
                self.identify.on_swarm_event(FromSwarm::NewListener(e));
            }
            FromSwarm::ExpiredListenAddr(e) => {
                self.identify
                    .on_swarm_event(FromSwarm::ExpiredListenAddr(e));
            }
            FromSwarm::NewExternalAddrCandidate(e) => {
                self.identify
                    .on_swarm_event(FromSwarm::NewExternalAddrCandidate(e));
            }
            FromSwarm::ExternalAddrConfirmed(e) => {
                self.identify
                    .on_swarm_event(FromSwarm::ExternalAddrConfirmed(e));
            }
            FromSwarm::AddressChange(e) => {
                self.identify.on_swarm_event(FromSwarm::AddressChange(e));
            }
            FromSwarm::NewListenAddr(e) => {
                self.identify.on_swarm_event(FromSwarm::NewListenAddr(e));
            }
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        self.identify
            .on_connection_handler_event(peer_id, connection_id, event)
    }

    fn poll(
        &mut self,
        cx: &mut Context,
        params: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        loop {
            match self.identify.poll(cx, params) {
                Poll::Pending => break,
                Poll::Ready(ToSwarm::GenerateEvent(event)) => match event {
                    identify::Event::Received { peer_id, info, .. } => {
                        let event = PeerInfoEvent::Identified { peer_id, info };
                        return Poll::Ready(ToSwarm::GenerateEvent(event));
                    }
                    identify::Event::Error { peer_id, error } => {
                        error!(?peer_id, %error, "Identification with peer failed")
                    }
                    identify::Event::Pushed { .. } => {}
                    identify::Event::Sent { .. } => {}
                },
                Poll::Ready(ToSwarm::Dial { opts }) => return Poll::Ready(ToSwarm::Dial { opts }),
                Poll::Ready(ToSwarm::NotifyHandler {
                    peer_id,
                    handler,
                    event,
                }) => {
                    return Poll::Ready(ToSwarm::NotifyHandler {
                        peer_id,
                        handler,
                        event,
                    })
                }
                Poll::Ready(ToSwarm::CloseConnection {
                    peer_id,
                    connection,
                }) => {
                    return Poll::Ready(ToSwarm::CloseConnection {
                        peer_id,
                        connection,
                    })
                }
                Poll::Ready(ToSwarm::NewExternalAddrCandidate(observed)) => {
                    return Poll::Ready(ToSwarm::NewExternalAddrCandidate(observed))
                }
                Poll::Ready(ToSwarm::ExternalAddrConfirmed(addr)) => {
                    return Poll::Ready(ToSwarm::ExternalAddrConfirmed(addr))
                }
                Poll::Ready(ToSwarm::ExternalAddrExpired(addr)) => {
                    return Poll::Ready(ToSwarm::ExternalAddrExpired(addr))
                }
                Poll::Ready(ToSwarm::ListenOn { opts }) => {
                    return Poll::Ready(ToSwarm::ListenOn { opts })
                }
                Poll::Ready(ToSwarm::RemoveListener { id }) => {
                    return Poll::Ready(ToSwarm::RemoveListener { id })
                }
            }
        }

        Poll::Pending
    }
}
