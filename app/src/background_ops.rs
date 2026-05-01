//! Shared lifecycle helpers for app background operations.

use tokio::sync::broadcast;

#[derive(Debug)]
pub(crate) enum BroadcastOperationEvent<T> {
    Idle,
    Empty,
    Ready { id: u64, reply: T },
    Lagged { skipped: u64 },
    Closed,
}

#[derive(Debug)]
struct ActiveBroadcastOperation<T> {
    id: u64,
    receiver: broadcast::Receiver<T>,
}

#[derive(Debug)]
pub(crate) struct BroadcastOperation<T> {
    next_id: u64,
    active: Option<ActiveBroadcastOperation<T>>,
}

impl<T: Clone> Default for BroadcastOperation<T> {
    fn default() -> Self {
        Self {
            next_id: 1,
            active: None,
        }
    }
}

impl<T: Clone> BroadcastOperation<T> {
    pub(crate) fn start(&mut self, receiver: broadcast::Receiver<T>) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1).max(1);
        self.active = Some(ActiveBroadcastOperation { id, receiver });
        id
    }

    pub(crate) fn is_in_flight(&self) -> bool {
        self.active.is_some()
    }

    pub(crate) fn poll(&mut self) -> BroadcastOperationEvent<T> {
        let Some(active) = self.active.as_mut() else {
            return BroadcastOperationEvent::Idle;
        };

        match active.receiver.try_recv() {
            Ok(reply) => BroadcastOperationEvent::Ready {
                id: active.id,
                reply,
            },
            Err(broadcast::error::TryRecvError::Empty) => BroadcastOperationEvent::Empty,
            Err(broadcast::error::TryRecvError::Lagged(skipped)) => {
                BroadcastOperationEvent::Lagged { skipped }
            }
            Err(broadcast::error::TryRecvError::Closed) => {
                self.active = None;
                BroadcastOperationEvent::Closed
            }
        }
    }

    pub(crate) fn complete_if_current(&mut self, id: u64) -> bool {
        if self.active.as_ref().is_some_and(|active| active.id == id) {
            self.active = None;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_stale_completion_after_newer_operation_starts() {
        let (sender, receiver) = broadcast::channel(4);
        let mut operation = BroadcastOperation::<u8>::default();
        let stale_id = operation.start(receiver);
        let current_id = operation.start(sender.subscribe());

        assert!(!operation.complete_if_current(stale_id));
        assert!(operation.is_in_flight());
        assert!(operation.complete_if_current(current_id));
        assert!(!operation.is_in_flight());
    }

    #[test]
    fn closed_channel_clears_in_flight_state() {
        let (sender, receiver) = broadcast::channel::<u8>(4);
        let mut operation = BroadcastOperation::default();
        operation.start(receiver);
        drop(sender);

        assert!(matches!(operation.poll(), BroadcastOperationEvent::Closed));
        assert!(!operation.is_in_flight());
    }

    #[test]
    fn lagged_channel_keeps_operation_active() {
        let (sender, receiver) = broadcast::channel(1);
        let mut operation = BroadcastOperation::default();
        operation.start(receiver);
        sender.send(1).expect("first send should succeed");
        sender.send(2).expect("second send should succeed");

        assert!(matches!(
            operation.poll(),
            BroadcastOperationEvent::Lagged { skipped: 1 }
        ));
        assert!(operation.is_in_flight());
    }
}
