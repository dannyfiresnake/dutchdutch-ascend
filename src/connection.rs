use crate::error::{AscendError, Result};
use crate::protocol::{Request, Response};
use crate::subscription::StateUpdate;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// The speakers answer RFC 6455 pings, so a connection that has gone quiet
/// can be told apart from one that has died without polling application
/// state for signs of life.
const PING_INTERVAL: Duration = Duration::from_secs(15);

/// Silence longer than this means the socket is dead even though nothing
/// reported an error -- a half-open TCP connection looks exactly like an
/// idle one until you ask it something.
const PONG_TIMEOUT: Duration = Duration::from_secs(45);

/// WebSocket connection state
struct ConnectionState {
    /// Pending requests waiting for responses
    pending_requests: HashMap<Uuid, oneshot::Sender<Response>>,
    /// Channel for sending outgoing messages
    ws_tx: mpsc::UnboundedSender<Message>,
}

/// Low-level WebSocket connection handler
pub struct Connection {
    state: Arc<Mutex<ConnectionState>>,
    /// Broadcast channel for subscription updates (outside mutex to allow non-blocking subscribe)
    subscription_tx: broadcast::Sender<StateUpdate>,
}

impl Connection {
    /// Connect to a WebSocket URL
    pub async fn connect(url: impl Into<String>) -> Result<Self> {
        let url = url.into();
        tracing::info!("Connecting to {}", url);

        let (ws_stream, _) = connect_async(&url).await?;
        let (mut write, mut read) = ws_stream.split();

        // Create channels
        let (ws_tx, mut ws_rx) = mpsc::unbounded_channel::<Message>();
        // Deep enough that a burst of meter updates does not make every
        // subscriber lag at once; lag is survivable now, but still lossy.
        let (subscription_tx, _) = broadcast::channel(1024);

        // Any traffic counts as proof of life, so this starts optimistic.
        let last_seen = Arc::new(std::sync::Mutex::new(std::time::Instant::now()));

        let state = Arc::new(Mutex::new(ConnectionState {
            pending_requests: HashMap::new(),
            ws_tx,
        }));

        // Spawn task to forward outgoing messages to WebSocket
        let write_handle = tokio::spawn(async move {
            while let Some(msg) = ws_rx.recv().await {
                if let Err(e) = write.send(msg).await {
                    tracing::error!("Failed to send message: {}", e);
                    break;
                }
            }
        });

        // Spawn task to receive and process incoming messages
        let state_clone = state.clone();
        let subscription_tx_clone = subscription_tx.clone();
        let last_seen_rx = last_seen.clone();
        tokio::spawn(async move {
            while let Some(msg_result) = read.next().await {
                *last_seen_rx.lock().unwrap() = std::time::Instant::now();
                match msg_result {
                    Ok(Message::Text(text)) => {
                        if let Err(e) = Self::handle_message(&state_clone, &subscription_tx_clone, text).await {
                            tracing::error!("Error handling message: {}", e);
                        }
                    }
                    Ok(Message::Close(_)) => {
                        tracing::info!("WebSocket connection closed");
                        break;
                    }
                    Err(e) => {
                        tracing::error!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }

            // Connection closed, cancel all pending requests
            let mut state = state_clone.lock().await;
            state.pending_requests.clear();
            drop(write_handle);
        });

        // Keepalive. Without it a half-open connection sits there looking
        // healthy: no error, no close frame, and no data ever again.
        let state_ping = state.clone();
        let last_seen_ping = last_seen;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(PING_INTERVAL);
            tick.tick().await;
            loop {
                tick.tick().await;
                let quiet = last_seen_ping.lock().unwrap().elapsed();
                let guard = state_ping.lock().await;
                if quiet > PONG_TIMEOUT {
                    tracing::warn!("no reply for {:?}, closing connection", quiet);
                    let _ = guard.ws_tx.send(Message::Close(None));
                    break;
                }
                if guard.ws_tx.send(Message::Ping(Vec::new())).is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            state,
            subscription_tx,
        })
    }

    /// Handle an incoming message
    async fn handle_message(
        state: &Arc<Mutex<ConnectionState>>,
        subscription_tx: &broadcast::Sender<StateUpdate>,
        text: String,
    ) -> Result<()> {
        tracing::debug!("Received: {}", text);

        let response: Response = serde_json::from_str(&text)?;

        let mut state = state.lock().await;

        // Check if this is a response to a pending request
        if let Some(tx) = state.pending_requests.remove(&response.meta.id) {
            // Send response to waiting request
            let _ = tx.send(response);
        } else {
            // This is a subscription update (no matching request ID)
            if let Some(update) = Self::parse_state_update(&response) {
                let _ = subscription_tx.send(update);
            }
        }

        Ok(())
    }

    /// Parse a response into a state update
    fn parse_state_update(response: &Response) -> Option<StateUpdate> {
        use crate::protocol::Method;

        // Check if this is a network subscription notification
        if response.meta.method == Method::Notify
            && response.meta.response_type.as_deref() == Some("network") {

            if let Some(data) = &response.data {
                // Look for data.state
                if let Some(state) = data.get("state") {
                    if let Some(state_obj) = state.as_object() {
                        // Find the first room in the state
                        for (_state_id, state_entry) in state_obj {
                            if let Some(entry_data) = state_entry.get("data") {
                                if entry_data.get("type").and_then(|v| v.as_str()) == Some("room") {
                                    // Return raw JSON for room updates
                                    return Some(StateUpdate::RoomUpdate(Box::new(entry_data.clone())));
                                }
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Send a request and wait for the response
    pub async fn send_request(&self, request: Request) -> Result<Response> {
        let request_id = request.id();
        let (tx, rx) = oneshot::channel();

        // Register the pending request
        {
            let mut state = self.state.lock().await;
            state.pending_requests.insert(request_id, tx);

            // Send the request
            let json = serde_json::to_string(&request)?;
            tracing::debug!("Sending: {}", json);

            state
                .ws_tx
                .send(Message::Text(json))
                .map_err(|_| AscendError::ConnectionClosed)?;
        }

        // Wait for response with timeout
        let response = match timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => return Err(AscendError::ConnectionClosed),
            Err(_) => {
                // Timeout - remove from pending requests
                let mut state = self.state.lock().await;
                state.pending_requests.remove(&request_id);
                return Err(AscendError::Timeout);
            }
        };

        // Check for API errors
        if response.has_errors() {
            if let Some(detail) = response.error_message() {
                return Err(AscendError::ApiError { detail });
            }
        }

        Ok(response)
    }

    /// Subscribe to state updates
    pub fn subscribe(&self) -> broadcast::Receiver<StateUpdate> {
        self.subscription_tx.subscribe()
    }

    /// Send a request without waiting for a response (fire and forget)
    pub async fn send_only(&self, request: Request) -> Result<()> {
        let state = self.state.lock().await;
        let json = serde_json::to_string(&request)?;
        tracing::debug!("Sending (no response): {}", json);

        state
            .ws_tx
            .send(Message::Text(json))
            .map_err(|_| AscendError::ConnectionClosed)?;

        Ok(())
    }
}
