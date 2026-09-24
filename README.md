# dutchdutch-ascend

A Rust library for controlling [Dutch and Dutch](https://dutchdutch.com/) networked speakers.

## Features

- **Discovery**: Automatic room discovery via Ascend Cloud API
- **Room Control**: Connect to and control speaker systems via local WebSocket
- **Volume & Mute**: Global and per-position volume/mute control
- **Audio Settings**:
  - Voicing profile selection
  - Tone adjustment (bass, treble, sub)
  - Linear phase control
  - Channel mapping configuration
- **Input Management**: Switch between input modes and XLR configurations
- **Presets**: Load and manage room presets
- **Real-time Updates**: Subscribe to live state changes from the speakers
- **Standby Control**: Put speakers in/out of standby mode

## Connection health

State arrives by subscription, not polling. Two things keep that trustworthy:

- **Ping keepalive.** The speakers answer RFC 6455 pings, so a connection that
  has merely gone quiet can be told apart from one that has died. A half-open
  socket otherwise looks exactly like an idle one: no error, no close frame,
  and no data ever again.
- **Lag is survived, not fatal.** A subscriber that falls behind gets
  `Lagged`, which means "you missed messages", not "the stream is gone".
  `StateReceiver::recv_resilient` keeps the two apart; discovery re-reads
  state once to cover the gap and carries on listening.

Treating `Lagged` as fatal used to strand the update task permanently: the
socket stayed up and requests kept working, while room state silently stopped
changing until the process was restarted.

- **Reconnection is ordinary, not exceptional.** Each speaker gets a
  supervisor that connects, reads state, subscribes, consumes until the stream
  ends, and then goes round again with backoff (1s doubling to 30s). Recovery
  is the same code path as the first connection.
- **Rooms are dropped when their connection dies**, and `RoomRemoved` says so.
  A `Room` holds the connection it was built from, so refreshing a stale one
  would leave it sending to a dead socket. Dropping it means any other
  connected speaker rebuilds it from its next notification -- every speaker
  reports every room -- and failing that, its own supervisor reconnects.

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
dutchdutch-ascend = "0.1.0"
```

## Quick Start

### Discovery Mode

The simplest way to get started is using the discovery API, which automatically finds Dutch and Dutch speakers on your network:

```rust
use dutchdutch_ascend::Discovery;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Start discovery
    let mut discovery = Discovery::new();
    discovery.start().await?;

    // Wait for rooms to be discovered
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Get discovered rooms
    let rooms = discovery.rooms();
    if let Some(discovered_room) = rooms.first() {
        println!("Found room: {}", discovered_room.name());

        // Connect to the room (already connected via discovery)
        let room = discovered_room;

        // Control the room
        room.set_gain(-20.0).await?;
        room.set_mute(false).await?;

        println!("Volume: {:.1} dB", room.gain().global);
        println!("Muted: {}", room.mute().global);
    }

    discovery.stop().await;
    Ok(())
}
```

### Direct Connection

If you know the IP address of your speaker, you can connect directly:

```rust
use dutchdutch_ascend::AscendClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = AscendClient::connect("192.168.1.100", 8768).await?;
    let rooms = client.rooms().await?;

    if let Some(room) = rooms.first() {
        room.set_gain(-15.0).await?;
        println!("Volume set to -15.0 dB");
    }

    Ok(())
}
```

### Real-time State Updates

Subscribe to state changes from the speakers:

```rust
use dutchdutch_ascend::Discovery;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut discovery = Discovery::new();
    let mut updates = discovery.subscribe_updates();

    discovery.start().await?;

    // Listen for room updates
    while let Ok(room_id) = updates.recv().await {
        println!("Room {} updated", room_id);

        // Get the updated room
        let rooms = discovery.rooms();
        if let Some(room) = rooms.iter().find(|r| r.id() == room_id) {
            println!("New volume: {:.1} dB", room.gain().global);
        }
    }

    Ok(())
}
```

## Interactive Example

The library includes a full-featured terminal UI example that demonstrates all functionality:

```bash
cargo run --example discover
```

This interactive application allows you to:
- Discover Dutch and Dutch speaker rooms on your network
- View room details and connected devices
- Control volume, mute, and standby
- Switch input modes and XLR configurations
- Toggle linear phase processing
- View real-time room state updates
- Inspect raw JSON protocol data

### Controls

**Discovery Screen:**
- `j`/`k` or arrow keys: Select room
- `Enter`: Connect to room
- `q`: Quit

**Room Control Screen:**
- `+`/`-`: Adjust volume
- `m`: Toggle mute
- `s`: Toggle standby
- `i`: Cycle input mode
- `x`: Cycle XLR mode
- `p`: Toggle linear phase
- `j`/`k`: Scroll JSON view
- `Esc`: Back to discovery
- `q`: Quit

## API Overview

### Discovery

```rust
let mut discovery = Discovery::new();
discovery.start().await?;
let rooms = discovery.rooms();
discovery.stop().await;
```

### Room Control

```rust
// Volume control
room.set_gain(-20.0).await?;
let current_volume = room.gain().global;

// Mute control
room.set_mute(true).await?;
let is_muted = room.mute().global;

// Standby control
room.set_standby(true).await?;
let is_asleep = room.sleep();

// Input selection
let inputs = room.input_modes();
room.set_input(&inputs[0]).await?;

// XLR mode selection
let xlr_modes = room.xlr_input_modes();
room.set_xlr_mode(&xlr_modes[0]).await?;

// Linear phase
room.set_linear_phase(true).await?;
let linear_phase = room.linear_phase();

// Voicing profiles
let voicings = room.voicing_profiles();
if let Some((id, profile)) = voicings.iter().next() {
    room.set_voicing_profile(id).await?;
}

// Room state snapshot
let state = room.state_snapshot();
println!("Room: {}", state.name);
println!("Volume: {:.1} dB", state.gain.global);
```

## Requirements

- Rust 1.70 or later
- Dutch and Dutch Ascend speaker system
- Network connectivity to speakers (local) and Ascend Cloud (for discovery)

## Protocol

This library communicates with Dutch and Dutch speakers using:
- WebSocket protocol over port 8768 (local speaker connection)
- JSON-based command and state messages
- Cloud discovery via `wss://api.ascend.audio/`

## License

MIT License - see [LICENSE](LICENSE) file for details.

## Disclaimer

This is an unofficial, community-developed library and is not affiliated with or endorsed by Dutch and Dutch. Use at your own risk.
