# How to run & verify

1. Clone this repo locally.

1. Start `Polkadot` node via `Docker` locally:

    ```sh
    docker run --platform linux/amd64 -p 30333:30333 parity/polkadot --name "Polkadot Node" --dev -l "info,libp2p_swarm=debug" --node-key 0x9aea8a9ebabefa48faa0fc52a784502958485c0cf568b97877db586ac40d1580
    ```
    
1. Wait until it completes startup initialization, there should be message in log like this:

    ```
    libp2p_swarm: Listener ListenerId(130333"
    ```
    
1. Open shell inside root folder of the repo and start test app:

    ```sh
    cargo run
    ```
    
1. Verify the output of the app, it should be like this:

    ```
    2023-08-15T12:42:50.372520Z  INFO polkadot_handshake: Local node identity id="12D3KooWR8hEf9UtFDFUbQvdJ4Bf4VEVSGNgPjgX7v74jHXnLw36"
    2023-08-15T12:42:50.395570Z  INFO polkadot_handshake: event=ConnectionEstablished { peer_id: PeerId("12D3KooWBzri3QhYjStNCTvhwrmLExYZjy29hfCG4tbjwixKfHCt"), connection_id: ConnectionId(1), endpoint: Dialer { address: "/ip4/127.0.0.1/tcp/30333/p2p/12D3KooWBzri3QhYjStNCTvhwrmLExYZjy29hfCG4tbjwixKfHCt", role_override: Dialer }, num_established: 1, concurrent_dial_errors: Some([]), established_in: 21.4485ms }
    2023-08-15T12:42:50.430049Z  INFO polkadot_handshake: event=Behaviour(Identified { peer_id: PeerId("12D3KooWBzri3QhYjStNCTvhwrmLExYZjy29hfCG4tbjwixKfHCt"), ... })
    2023-08-15T12:42:50.610247Z  INFO polkadot_handshake: event=ConnectionClosed { peer_id: PeerId("12D3KooWBzri3QhYjStNCTvhwrmLExYZjy29hfCG4tbjwixKfHCt"), connection_id: ConnectionId(1), endpoint: Dialer { address: "/ip4/127.0.0.1/tcp/30333/p2p/12D3KooWBzri3QhYjStNCTvhwrmLExYZjy29hfCG4tbjwixKfHCt", role_override: Dialer }, num_established: 0, cause: Some(KeepAliveTimeout) }
    ```
    
    Notice, our test app successfully established connection to
    `12D3KooWBzri3QhYjStNCTvhwrmLExYZjy29hfCG4tbjwixKfHCt` peer (our
    `Polkadot` node).
    
1. Verify the output of `Polkadot` node, it should be like this:

    ```
    2023-08-15 12:42:50.407 DEBUG tokio-runtime-worker libp2p_swarm: Connection established: PeerId("12D3KooWR8hEf9UtFDFUbQvdJ4Bf4VEVSGNgPjgX7v74jHXnLw36") Listener { local_addr: "/ip4/172.17.0.2/tcp/30333", send_back_addr: "/ip4/172.17.0.1/tcp/50264" }; Total (peer): 1
    ```
    
    Notice, our `Polkadot` node connected to our test app.
