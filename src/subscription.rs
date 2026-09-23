use crate::error::{AscendError, Result};
use crate::types::{Device, DeviceId};
use tokio::sync::broadcast;

/// State update from a subscription
#[derive(Debug, Clone)]
pub enum StateUpdate {
    /// Room state was updated (raw JSON)
    RoomUpdate(Box<serde_json::Value>),

    /// Device state was updated
    DeviceUpdate(DeviceId, Device),
}

/// What a resilient receive produced.
#[derive(Debug, Clone)]
pub enum RecvOutcome {
    /// A state update from the speakers.
    Update(StateUpdate),
    /// The receiver fell behind and this many messages were dropped. The
    /// stream is still live: the right response is to resync and keep
    /// listening, not to stop.
    Lagged(u64),
}

/// Receiver for state updates
pub struct StateReceiver {
    rx: broadcast::Receiver<StateUpdate>,
}

impl StateReceiver {
    /// Create a new state receiver
    pub(crate) fn new(rx: broadcast::Receiver<StateUpdate>) -> Self {
        Self { rx }
    }

    /// Receive the next state update
    ///
    /// Returns `None` if all senders have been dropped (connection closed).
    pub async fn recv(&mut self) -> Result<StateUpdate> {
        self.rx
            .recv()
            .await
            .map_err(|e| match e {
                broadcast::error::RecvError::Closed => AscendError::ConnectionClosed,
                broadcast::error::RecvError::Lagged(n) => {
                    AscendError::ChannelError(format!("Lagged by {} messages", n))
                }
            })
    }

    /// Receive, keeping "I missed messages" apart from "the stream is gone".
    ///
    /// `recv` collapses both into an error, which makes it easy to abandon a
    /// perfectly live subscription after one burst -- the speakers push a live
    /// input meter, so bursts are normal. Only a closed channel ends the
    /// stream here.
    pub async fn recv_resilient(&mut self) -> Result<RecvOutcome> {
        match self.rx.recv().await {
            Ok(update) => Ok(RecvOutcome::Update(update)),
            Err(broadcast::error::RecvError::Lagged(n)) => Ok(RecvOutcome::Lagged(n)),
            Err(broadcast::error::RecvError::Closed) => Err(AscendError::ConnectionClosed),
        }
    }

    /// Try to receive a state update without blocking
    ///
    /// Returns `None` if no message is available.
    pub fn try_recv(&mut self) -> Result<Option<StateUpdate>> {
        match self.rx.try_recv() {
            Ok(update) => Ok(Some(update)),
            Err(broadcast::error::TryRecvError::Empty) => Ok(None),
            Err(broadcast::error::TryRecvError::Closed) => Err(AscendError::ConnectionClosed),
            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                Err(AscendError::ChannelError(format!("Lagged by {} messages", n)))
            }
        }
    }
}
