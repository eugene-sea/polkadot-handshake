use std::{num::NonZeroUsize, time::Duration};

use libp2p::{
    core::{muxing::StreamMuxerBox, transport::Boxed, upgrade},
    futures::StreamExt,
    identify,
    identity::{self, PublicKey},
    swarm::{NetworkBehaviour, StreamUpgradeError, SwarmBuilder},
    tcp, Multiaddr, PeerId, Transport,
};
use tracing::{error, info};

mod noise;

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

#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "PeerInfoEvent")]
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
#[allow(clippy::large_enum_variant)]
pub enum PeerInfoEvent {
    /// We have obtained identity information from a peer, including the addresses it is listening
    /// on
    Identified {
        /// Id of the peer that has been identified
        peer_id: PeerId,
        /// Information about the peer
        info: identify::Info,
    },
    Error {
        peer_id: PeerId,
        error: StreamUpgradeError<identify::UpgradeError>,
    },
    Uninteresting,
}

impl From<identify::Event> for PeerInfoEvent {
    fn from(event: identify::Event) -> Self {
        match event {
            identify::Event::Received { peer_id, info, .. } => {
                PeerInfoEvent::Identified { peer_id, info }
            }
            identify::Event::Error { peer_id, error } => {
                error!(?peer_id, %error, "Identification with peer failed");
                PeerInfoEvent::Error { peer_id, error }
            }
            identify::Event::Pushed { .. } => PeerInfoEvent::Uninteresting,
            identify::Event::Sent { .. } => PeerInfoEvent::Uninteresting,
        }
    }
}
