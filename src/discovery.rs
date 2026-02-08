use crate::error::{AscendError, Result};
use crate::room::Room;
use crate::speaker_connection::SpeakerConnection;
use crate::types::RoomId;
use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

const MDNS_SERVICE_TYPE: &str = "_x-clerk._tcp.local.";
const SPEAKER_PORT: u16 = 8768;

/// Events emitted by Discovery when rooms are discovered, updated, or removed
#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    /// A new room was discovered and added
    RoomAdded(RoomId),
    /// An existing room's state was updated
    RoomUpdated(RoomId),
    /// A room was removed (speaker disconnected)
    RoomRemoved(RoomId),
}

/// Discovery manager for Ascend speakers
///
/// Manages the discovery process and maintains a persistent list of discovered rooms.
/// The discovery process runs in the background and automatically reconnects if the
/// connection is lost.
///
/// # Example
///
/// ```no_run
/// use dutchdutch_ascend::Discovery;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let mut discovery = Discovery::new();
///     discovery.start().await?;
///
///     // Wait a bit for discovery
///     tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
///
///     for room in discovery.rooms() {
///         println!("Found room: {} at {}", room.name, room.master_ip);
///     }
///
///     discovery.stop().await;
///     Ok(())
/// }
/// ```
pub struct Discovery {
    speakers: Arc<Mutex<BTreeMap<String, Arc<SpeakerConnection>>>>,
    rooms: Arc<Mutex<BTreeMap<RoomId, Room>>>,
    event_tx: Arc<broadcast::Sender<DiscoveryEvent>>,
    mdns_daemon: Option<ServiceDaemon>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl Discovery {
    /// Create a new Discovery manager
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(100);
        Self {
            speakers: Arc::new(Mutex::new(BTreeMap::new())),
            rooms: Arc::new(Mutex::new(BTreeMap::new())),
            event_tx: Arc::new(event_tx),
            mdns_daemon: None,
            task_handle: None,
        }
    }

    /// Subscribe to discovery events
    ///
    /// Returns a receiver that will receive DiscoveryEvent messages when rooms are added, updated, or removed
    pub fn subscribe(&self) -> broadcast::Receiver<DiscoveryEvent> {
        self.event_tx.subscribe()
    }

    /// Get a snapshot of currently discovered rooms
    pub fn rooms(&self) -> Vec<Room> {
        let rooms = self.rooms.lock().unwrap();
        rooms.values().cloned().collect()
    }

    /// Get the number of discovered rooms
    pub fn room_count(&self) -> usize {
        let rooms = self.rooms.lock().unwrap();
        rooms.len()
    }

    /// Start the discovery process
    ///
    /// This begins mDNS discovery of speakers on the local network.
    /// The discovery runs in the background and will continuously discover
    /// new speakers as they appear on the network.
    pub async fn start(&mut self) -> Result<()> {
        // Stop existing discovery if running
        self.stop().await;

        tracing::info!("Starting mDNS discovery for service type: {}", MDNS_SERVICE_TYPE);

        // Create a daemon
        let mdns = ServiceDaemon::new()
            .map_err(|e| AscendError::InvalidResponse(format!("Failed to create mDNS daemon: {}", e)))?;

        // Browse for services
        let receiver = mdns.browse(MDNS_SERVICE_TYPE)
            .map_err(|e| AscendError::InvalidResponse(format!("Failed to browse for services: {}", e)))?;

        self.mdns_daemon = Some(mdns);

        let speakers = self.speakers.clone();
        let rooms = self.rooms.clone();
        let event_tx = self.event_tx.clone();

        // Spawn background task to process mDNS events
        let handle = tokio::spawn(async move {
            loop {
                match receiver.recv_async().await {
                    Ok(ServiceEvent::ServiceResolved(info)) => {
                        tracing::info!("Discovered service: {}", info.get_fullname());

                        // Process each address from this service
                        for addr in info.get_addresses() {
                            let ip_str = addr.to_string();
                            tracing::info!("  Address: {}", ip_str);

                            // Check if we already have this speaker
                            let already_connected = {
                                let speakers_lock = speakers.lock().unwrap();
                                speakers_lock.contains_key(&ip_str)
                            };

                            if already_connected {
                                tracing::debug!("Speaker at {} already connected, skipping", ip_str);
                                continue;
                            }

                            // Process the newly discovered speaker
                            if let Err(e) = process_speaker(&ip_str, &speakers, &rooms, &event_tx).await {
                                tracing::warn!("Failed to process speaker at {}: {}", ip_str, e);
                            }
                        }
                    }
                    Ok(ServiceEvent::SearchStopped(_)) => {
                        tracing::info!("mDNS search stopped");
                        break;
                    }
                    Ok(_) => {
                        // Other event types - ignore
                    }
                    Err(e) => {
                        tracing::error!("mDNS receiver error: {}", e);
                        break;
                    }
                }
            }
        });

        self.task_handle = Some(handle);
        Ok(())
    }

    /// Stop the discovery process
    ///
    /// The room list is preserved and can be accessed after stopping.
    pub async fn stop(&mut self) {
        // Shutdown the mDNS daemon
        if let Some(mdns) = self.mdns_daemon.take() {
            mdns.shutdown().ok();
        }

        // Wait for the background task to finish
        if let Some(handle) = self.task_handle.take() {
            let _ = tokio::time::timeout(tokio::time::Duration::from_millis(500), handle).await;
        }
    }

    /// Update or add a room from JSON data
    ///
    /// Parses the room state from the JSON, updates it if it exists, or adds it if new.
    /// Sends appropriate events (RoomAdded/RoomUpdated) on the event channel.
    fn update_room(
        rooms: &Arc<Mutex<BTreeMap<RoomId, Room>>>,
        event_tx: &Arc<broadcast::Sender<DiscoveryEvent>>,
        speaker: &Arc<SpeakerConnection>,
        room_json: serde_json::Value,
    ) {
        // Extract room ID from JSON
        let room_id = match room_json.get("id")
            .and_then(|v| v.as_str())
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
        {
            Some(id) => id,
            None => {
                tracing::warn!("Received room data without valid ID");
                return;
            }
        };

        let mut rooms_lock = rooms.lock().unwrap();

        if let Some(existing_room) = rooms_lock.get(&room_id) {
            // Update existing room
            match existing_room.update_from_json(room_json) {
                Ok(changed) => {
                    if changed {
                        tracing::debug!("Room state changed: {}", room_id);
                        let _ = event_tx.send(DiscoveryEvent::RoomUpdated(room_id));
                    } else {
                        tracing::trace!("Room state unchanged: {}", room_id);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to update room {}: {}", room_id, e);
                }
            }
        } else {
            // New room discovered
            match Room::new(speaker.clone(), room_json) {
                Ok(new_room) => {
                    let room_name = new_room.name();
                    tracing::info!("New room discovered: {} ({})", room_name, room_id);
                    rooms_lock.insert(room_id, new_room);
                    let _ = event_tx.send(DiscoveryEvent::RoomAdded(room_id));
                }
                Err(e) => {
                    tracing::warn!("Failed to create room {}: {}", room_id, e);
                }
            }
        }
    }
}

impl Default for Discovery {
    fn default() -> Self {
        Self::new()
    }
}

/// Process a single speaker: connect, get network state, subscribe, and add rooms
async fn process_speaker(
    speaker_ip: &str,
    speakers: &Arc<Mutex<BTreeMap<String, Arc<SpeakerConnection>>>>,
    rooms: &Arc<Mutex<BTreeMap<RoomId, Room>>>,
    event_tx: &Arc<broadcast::Sender<DiscoveryEvent>>,
) -> Result<()> {
    tracing::info!("Processing speaker at {}", speaker_ip);

    // Check if we already have a connection to this speaker
    let speaker = {
        let speakers_lock = speakers.lock().unwrap();
        if let Some(existing) = speakers_lock.get(speaker_ip) {
            tracing::debug!("Reusing existing connection to {}", speaker_ip);
            Some(existing.clone())
        } else {
            None
        }
    };

    let speaker = if let Some(sp) = speaker {
        sp
    } else {
        // Create new connection (outside of lock)
        tracing::info!("Creating new connection to {}", speaker_ip);
        let conn = SpeakerConnection::connect(speaker_ip.to_string(), SPEAKER_PORT).await?;
        let arc_conn = Arc::new(conn);

        // Insert into map
        {
            let mut speakers_lock = speakers.lock().unwrap();
            speakers_lock.insert(speaker_ip.to_string(), arc_conn.clone());
        }

        arc_conn
    };

    // Request network state
    let network_data = match speaker.request_network_state().await {
        Ok(data) => data,
        Err(e) => {
            tracing::warn!("Failed to get network state from {}: {}", speaker_ip, e);
            return Err(e);
        }
    };

    // Parse rooms from network state and update/add them
    if let Err(e) = parse_and_update_rooms(&network_data, &speaker, rooms, event_tx) {
        tracing::warn!("Failed to parse rooms from network data: {}", e);
        return Err(e);
    }

    // Subscribe to state updates and spawn background task to process them
    match speaker.subscribe_network_state().await {
        Ok(mut receiver) => {
            let rooms_clone = rooms.clone();
            let event_tx_clone = event_tx.clone();
            let speaker_clone = speaker.clone();

            tokio::spawn(async move {
                while let Ok(update) = receiver.recv().await {
                    process_state_update(update, &speaker_clone, &rooms_clone, &event_tx_clone).await;
                }
                tracing::debug!("State update receiver closed for speaker");
            });
        }
        Err(e) => {
            tracing::warn!("Failed to subscribe to updates from {}: {}", speaker_ip, e);
        }
    }

    Ok(())
}

/// Process a state update from a speaker
async fn process_state_update(
    update: crate::subscription::StateUpdate,
    speaker: &Arc<SpeakerConnection>,
    rooms: &Arc<Mutex<BTreeMap<RoomId, Room>>>,
    event_tx: &Arc<broadcast::Sender<DiscoveryEvent>>,
) {
    match update {
        crate::subscription::StateUpdate::RoomUpdate(room_json) => {
            tracing::debug!("Received room update");
            Discovery::update_room(rooms, event_tx, speaker, *room_json);
        }
        _ => {
            // Other update types - ignore for now
        }
    }
}

/// Parse rooms from network state data and update the rooms map
fn parse_and_update_rooms(
    data: &serde_json::Value,
    speaker: &Arc<SpeakerConnection>,
    rooms: &Arc<Mutex<BTreeMap<RoomId, Room>>>,
    event_tx: &Arc<broadcast::Sender<DiscoveryEvent>>,
) -> Result<()> {
    tracing::debug!("Parsing rooms from network data");

    // The network endpoint returns data.state as a dictionary
    let state = data
        .get("state")
        .ok_or_else(|| AscendError::InvalidResponse("No state in network response".to_string()))?;

    let state_obj = state
        .as_object()
        .ok_or_else(|| AscendError::InvalidResponse("State is not an object".to_string()))?;

    tracing::debug!("Found {} state entries", state_obj.len());

    for (state_id, state_entry) in state_obj {
        // Check if this is a room
        let data_obj = match state_entry.get("data") {
            Some(d) => d,
            None => continue,
        };

        let type_str = match data_obj.get("type").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => continue,
        };

        if type_str != "room" {
            tracing::debug!("State entry {} is type {}, skipping", state_id, type_str);
            continue;
        }

        // This is a room, update or add it
        Discovery::update_room(rooms, event_tx, speaker, data_obj.clone());
    }

    Ok(())
}
