//! Rust library for controlling Dutch and Dutch Ascend networked speakers
//!
//! This library provides an async API for discovering and controlling Dutch and Dutch
//! Ascend speaker systems. It supports:
//!
//! - Discovery via mDNS (local network discovery)
//! - Room control via local WebSocket connection
//! - Volume and mute control (global and per-position)
//! - Voicing profile selection and tone adjustment
//! - Preset management
//! - Channel mapping configuration
//! - Real-time state update subscriptions
//!
//! # Quick Start
//!
//! ```no_run
//! use dutchdutch_ascend::Discovery;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Start discovery
//!     let mut discovery = Discovery::new();
//!     discovery.start().await?;
//!
//!     // Wait for rooms to be discovered
//!     tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
//!
//!     // Subscribe to discovery events
//!     let mut events = discovery.subscribe();
//!
//!     // Get discovered rooms
//!     let rooms = discovery.rooms();
//!     if let Some(room) = rooms.first() {
//!         println!("Found room: {}", room.name());
//!
//!         // Control the room directly
//!         room.set_gain(-20.0).await?;
//!         room.set_mute(false).await?;
//!     }
//!
//!     // Listen for room updates
//!     if let Ok(event) = events.recv().await {
//!         println!("Discovery event: {:?}", event);
//!     }
//!
//!     discovery.stop().await;
//!     Ok(())
//! }
//! ```
//!
//! # Direct Connection
//!
//! If you know the IP address of a speaker, you can connect directly:
//!
//! ```no_run
//! use dutchdutch_ascend::AscendClient;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = AscendClient::connect("192.168.1.100", 8768).await?;
//!     let rooms = client.rooms().await?;
//!     if let Some(room) = rooms.first() {
//!         room.set_gain(-15.0).await?;
//!     }
//!     Ok(())
//! }
//! ```
//!
//! # Architecture
//!
//! The library is organized into several layers:
//!
//! - **Discovery**: mDNS-based local network discovery of speakers
//! - **Client**: Direct connection to speakers when IP is known
//! - **Room**: High-level control API for speaker systems
//! - **Connection**: Low-level WebSocket protocol handling
//! - **Protocol**: JSON message structures
//! - **Types**: Domain types and data structures

mod client;
mod connection;
pub mod discovery;
mod error;
mod protocol;
mod room;
mod speaker_connection;
mod subscription;
mod types;

// Public exports
pub use client::AscendClient;
pub use discovery::{Discovery, DiscoveryEvent};
pub use error::{AscendError, Result};
pub use room::{Room, RoomState};
pub use subscription::{RecvOutcome, StateReceiver, StateUpdate};
pub use types::{
    ChannelGains, ChannelMapping, Device, DeviceId, GainData, GainLimits,
    GainValue, MuteData, MuteState, PositionId, Preset, RoomId, StreamingApi,
    StreamingApiMethod, StreamingInfo, ToneSettings, VoicingProfile,
};
