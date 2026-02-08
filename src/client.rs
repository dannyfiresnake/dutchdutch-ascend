use crate::error::{AscendError, Result};
use crate::room::Room;
use crate::speaker_connection::SpeakerConnection;
use crate::subscription::StateReceiver;
use std::sync::Arc;

/// Client for connecting to Dutch and Dutch Ascend speakers
///
/// The `AscendClient` manages the WebSocket connection to an Ascend speaker system
/// and provides access to room controls and state subscriptions.
pub struct AscendClient {
    speaker: Arc<SpeakerConnection>,
}

impl AscendClient {
    /// Connect directly to a speaker at the given IP address and port
    ///
    /// This establishes a WebSocket connection to the speaker's local API.
    /// The default port is 8768.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use dutchdutch_ascend::AscendClient;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let client = AscendClient::connect("192.168.1.100", 8768).await?;
    ///     let rooms = client.rooms().await?;
    ///     if let Some(room) = rooms.first() {
    ///         room.set_gain(-20.0).await?;
    ///     }
    ///     Ok(())
    /// }
    /// ```
    pub async fn connect(master_ip: impl Into<String>, port: u16) -> Result<Self> {
        let speaker = SpeakerConnection::connect(master_ip.into(), port).await?;

        Ok(Self {
            speaker: Arc::new(speaker),
        })
    }

    /// Get Room interfaces for all rooms in the speaker system
    ///
    /// This fetches the current network state and returns a vector of
    /// `Room` instances that can be used to control volume, mute, voicing, etc.
    pub async fn rooms(&self) -> Result<Vec<Room>> {
        // Get network state from speaker
        let data = self.speaker.request_network_state().await?;

        // Parse the state to find rooms
        let state = data
            .get("state")
            .ok_or_else(|| AscendError::InvalidResponse("No state in network response".to_string()))?;

        let state_obj = state
            .as_object()
            .ok_or_else(|| AscendError::InvalidResponse("State is not an object".to_string()))?;

        // Find all room entries (where data.type == "room")
        let mut rooms = Vec::new();
        for (_state_id, state_entry) in state_obj {
            if let Some(entry_data) = state_entry.get("data") {
                if entry_data.get("type").and_then(|v| v.as_str()) == Some("room") {
                    match Room::new(self.speaker.clone(), entry_data.clone()) {
                        Ok(room) => {
                            rooms.push(room);
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse room: {}", e);
                        }
                    }
                }
            }
        }

        if rooms.is_empty() {
            return Err(AscendError::InvalidResponse("No rooms found in network state".to_string()));
        }

        Ok(rooms)
    }

    /// Subscribe to state updates from the speaker system
    ///
    /// Returns a receiver that will yield state updates as they occur.
    /// Multiple subscriptions can be active simultaneously.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use dutchdutch_ascend::AscendClient;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let client = AscendClient::connect("192.168.1.100", 8768).await?;
    ///     let mut rx = client.subscribe_state().await?;
    ///
    ///     while let Ok(update) = rx.recv().await {
    ///         println!("State update: {:?}", update);
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn subscribe_state(&self) -> Result<StateReceiver> {
        self.speaker.subscribe_network_state().await
    }
}
