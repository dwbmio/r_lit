# Local Network Discovery in Murmur

## Overview

Murmur uses **iroh's native LocalSwarmDiscovery** for automatic peer discovery on local networks. This provides zero-configuration peer discovery similar to mDNS but integrated directly into iroh's networking stack.

## Architecture

### Discovery Flow

```
Node A starts → LocalSwarmDiscovery broadcasts presence
                ↓
Node B receives broadcast → Adds Node A to NodeMap
                ↓
Application calls discover_and_connect_local_peers()
                ↓
Iterates through remote_info_iter() → Connects to discovered peers
```

### Key Components

1. **LocalSwarmDiscovery** (iroh-net)
   - Automatically broadcasts node presence on local network
   - Listens for other nodes' broadcasts
   - Maintains NodeMap of discovered peers

2. **Network::discover_and_connect_peers()**
   - Queries `endpoint.remote_info_iter()` for discovered peers
   - Filters out self and already-connected peers
   - Attempts connection to each discovered peer

3. **ALPN Protocol Negotiation**
   - Client: Connects with `b"murmur"` ALPN
   - Server: Accepts with ServerConfig configured for `b"murmur"` ALPN
   - Both sides must agree on ALPN for successful connection

## Usage

### Basic Discovery

```rust
// Create and start swarm (LocalSwarmDiscovery starts automatically)
let swarm = Swarm::builder()
    .group_id("my-group")
    .build()
    .await?;

swarm.start().await?;

// Wait for discovery (iroh broadcasts in background)
tokio::time::sleep(Duration::from_secs(5)).await;

// Connect to discovered peers
let count = swarm.discover_and_connect_local_peers().await?;
println!("Connected to {} peers", count);
```

### Periodic Discovery

```rust
// Background task for continuous discovery
tokio::spawn(async move {
    loop {
        tokio::time::sleep(Duration::from_secs(10)).await;

        match swarm.discover_and_connect_local_peers().await {
            Ok(count) if count > 0 => {
                println!("Connected to {} new peers", count);
            }
            Ok(_) => {} // No new peers
            Err(e) => eprintln!("Discovery error: {}", e),
        }
    }
});
```

## Technical Details

### Why Not mDNS?

We initially tried using the `mdns-sd` crate but encountered critical issues:

**Problem 1: No Broadcasts Detected**
- `mdns-sd` failed to broadcast services on macOS
- System `dns-sd` tool could not detect broadcasts from `mdns-sd`
- Likely due to library-level issues with mDNS implementation

**Problem 2: Platform Compatibility**
- mDNS behavior varies across platforms (macOS, Linux, Windows)
- Requires system-level mDNS responder to be running
- Additional dependency and potential failure point

**Solution: iroh's LocalSwarmDiscovery**
- Native to iroh, no external dependencies
- Proven to work across platforms
- Integrated with iroh's networking stack
- Automatic and transparent

### ALPN Configuration

**Critical Requirement:** Both client and server must use the same ALPN protocol.

#### Client Side (Outgoing Connections)

```rust
let conn = endpoint.connect(peer_addr, b"murmur").await?;
```

#### Server Side (Incoming Connections)

```rust
// Create ServerConfig with ALPN
let alpn_protocols = vec![b"murmur".to_vec()];
let transport_config = Arc::new(TransportConfig::default());
let server_config = iroh_net::endpoint::make_server_config(
    &secret_key,
    alpn_protocols,
    transport_config,
    false,
)?;

// Accept with ServerConfig
let connecting = incoming.accept_with(server_config)?;
let conn = connecting.await?;
```

**Common Error:**
```
ERROR Failed to accept connection: the cryptographic handshake failed:
error 120: peer doesn't support any known protocol
```

This error means ALPN mismatch - the server didn't configure ALPN support.

### Type Compatibility

**Important:** Use types from `iroh_net::endpoint`, not direct `quinn` imports:

```rust
use iroh_net::endpoint::{ServerConfig, TransportConfig};

// ❌ Wrong - version conflicts
use quinn::{ServerConfig, TransportConfig};

// ✅ Correct - uses iroh's re-exported types
use iroh_net::endpoint::{ServerConfig, TransportConfig};
```

iroh uses `iroh-quinn` (a fork), not standard `quinn`. Using the wrong version causes type mismatches.

## Troubleshooting

### Peers Not Discovered

**Check 1: Wait Longer**
- Discovery takes 2-5 seconds
- Increase wait time before calling `discover_and_connect_local_peers()`

**Check 2: Same Network**
- Nodes must be on the same local network
- Check firewall rules (UDP traffic must be allowed)

**Check 3: Logs**
```
INFO inserting new node in NodeMap node=<node_id> source=local.swarm.discovery
```
This log confirms discovery is working.

### Connection Fails After Discovery

**Check 1: ALPN Configuration**
- Verify ServerConfig includes `b"murmur"` in alpn_protocols
- Check for error 120 in logs

**Check 2: Firewall**
- QUIC uses UDP, ensure UDP ports are open
- iroh uses relay servers as fallback

**Check 3: Connection Logs**
```
INFO new connection type typ=direct(<ip>:<port>)
```
This confirms connection attempt.

### ALPN Error 120

**Symptom:**
```
ERROR Failed to accept connection: error 120: peer doesn't support any known protocol
```

**Solution:**
1. Ensure `make_server_config()` is called with correct ALPN
2. Pass `server_config` to `incoming.accept_with()`
3. Don't use `incoming.accept()` without config

## Performance Considerations

### Discovery Timing

- **Initial Discovery**: 5-10 seconds recommended
- **Periodic Discovery**: 10-30 seconds interval
- **Network Overhead**: Minimal (UDP broadcasts)

### Connection Pooling

- Murmur maintains a `HashMap<NodeId, Connection>`
- Duplicate connections are automatically prevented
- Disconnected peers are removed automatically

### Scalability

- LocalSwarmDiscovery is designed for local networks (< 100 nodes)
- For larger deployments, consider relay-based discovery
- iroh supports multiple discovery mechanisms simultaneously

## Future Improvements

1. **Relay-Based Discovery**
   - Add PkarrPublisher for internet-wide discovery
   - Combine with LocalSwarmDiscovery using ConcurrentDiscovery

2. **Group Filtering**
   - Currently all nodes on network are discovered
   - Could add group-based filtering at application layer

3. **Discovery Events**
   - Stream of discovery events instead of polling
   - Real-time notification of new peers

## References

- [iroh-net Discovery Documentation](https://docs.rs/iroh-net/latest/iroh_net/discovery/)
- [LocalSwarmDiscovery Source](https://github.com/n0-computer/iroh/tree/main/iroh-net/src/discovery/local_swarm_discovery)
- [QUIC ALPN Specification](https://www.rfc-editor.org/rfc/rfc9001.html#name-alpn)
