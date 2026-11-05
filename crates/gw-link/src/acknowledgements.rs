use std::collections::{HashMap, VecDeque};
use tokio::sync::oneshot;

#[derive(Default)]
pub(super) struct PublishAcknowledgements {
    queued: VecDeque<oneshot::Sender<()>>,
    inflight: HashMap<u16, oneshot::Sender<()>>,
}

impl PublishAcknowledgements {
    pub fn enqueue(&mut self, waiter: oneshot::Sender<()>) {
        self.queued.push_back(waiter);
    }

    pub fn outgoing(&mut self, packet_id: u16) {
        if self.inflight.contains_key(&packet_id) {
            return;
        }
        // Retain timed-out queue entries: their wire packet still occupies this position.
        if let Some(waiter) = self.queued.pop_front() {
            self.inflight.insert(packet_id, waiter);
        }
    }

    pub fn acknowledge(&mut self, packet_id: u16) {
        if let Some(waiter) = self.inflight.remove(&packet_id) {
            let _ = waiter.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn enqueue_and_wire_send_do_not_confirm_delivery() {
        let mut tracker = PublishAcknowledgements::default();
        let (tx, mut rx) = oneshot::channel();
        tracker.enqueue(tx);
        tracker.outgoing(7);
        assert!(rx.try_recv().is_err());
        tracker.acknowledge(7);
        assert!(rx.await.is_ok());
    }

    #[tokio::test]
    async fn retransmission_does_not_consume_the_next_publish() {
        let mut tracker = PublishAcknowledgements::default();
        let (first, received_first) = oneshot::channel();
        let (second, mut received_second) = oneshot::channel();
        tracker.enqueue(first);
        tracker.outgoing(7);
        tracker.enqueue(second);
        tracker.outgoing(7);
        tracker.acknowledge(7);
        assert!(received_first.await.is_ok());
        assert!(received_second.try_recv().is_err());
        tracker.outgoing(8);
        tracker.acknowledge(8);
        assert!(received_second.await.is_ok());
    }

    #[tokio::test]
    async fn timeout_keeps_packet_order_and_cannot_confirm_another_message() {
        let mut tracker = PublishAcknowledgements::default();
        let (expired, expired_receiver) = oneshot::channel();
        tracker.enqueue(expired);
        drop(expired_receiver);
        let (next, mut receiver) = oneshot::channel();
        tracker.enqueue(next);
        tracker.outgoing(7);
        tracker.acknowledge(7);
        assert!(receiver.try_recv().is_err());
        tracker.outgoing(8);
        tracker.acknowledge(8);
        assert!(receiver.await.is_ok());
    }
}
