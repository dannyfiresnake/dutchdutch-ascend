use crate::connection::Connection;
use crate::error::Result;
use crate::protocol::{Method, Request};
use crate::subscription::StateReceiver;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

const MAX_CONNECT_RETRIES: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_secs(2);

/// Connection to a specific speaker
pub struct SpeakerConnection {
    ip: String,
    port: u16,
    connection: Arc<Connection>,
}

impl SpeakerConnection {
    /// Connect to a speaker at the given IP and port
    ///
    /// This will automatically retry the connection a few times if it fails.
    pub async fn connect(ip: String, port: u16) -> Result<Self> {
        let url = if ip.contains(':') {
            format!("ws://[{}]:{}", ip, port)
        } else {
            format!("ws://{}:{}", ip, port)
        };

        let mut last_error = None;

        for attempt in 1..=MAX_CONNECT_RETRIES {
            match Connection::connect(&url).await {
                Ok(connection) => {
                    tracing::info!("Connected to speaker at {} (attempt {})", ip, attempt);
                    return Ok(Self {
                        ip,
                        port,
                        connection: Arc::new(connection),
                    });
                }
                Err(e) => {
                    tracing::warn!("Failed to connect to {} (attempt {}/{}): {}",
                                 ip, attempt, MAX_CONNECT_RETRIES, e);
                    last_error = Some(e);

                    if attempt < MAX_CONNECT_RETRIES {
                        sleep(RETRY_DELAY).await;
                    }
                }
            }
        }

        Err(last_error.unwrap())
    }

    /// Get the speaker's IP address
    pub fn ip(&self) -> &str {
        &self.ip
    }

    /// Get the speaker's port
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get the underlying connection
    pub fn connection(&self) -> Arc<Connection> {
        self.connection.clone()
    }

    /// Request network state from the speaker
    pub async fn request_network_state(&self) -> Result<serde_json::Value> {
        let request = Request::new("network", Method::Read);
        let response = self.connection.send_request(request).await?;

        response
            .data
            .ok_or_else(|| crate::error::AscendError::InvalidResponse("No data in network response".to_string()))
    }

    /// Subscribe to state updates from the speaker
    pub async fn subscribe_network_state(&self) -> Result<StateReceiver> {
        let request = Request::new("network", Method::Subscribe);
        self.connection.send_only(request).await?;

        let rx = self.connection.subscribe();
        Ok(StateReceiver::new(rx))
    }
}
