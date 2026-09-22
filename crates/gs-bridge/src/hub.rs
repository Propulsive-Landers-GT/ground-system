//! Fan-out of server messages to every WebSocket client, plus the state a newly
//! connected client needs to catch up.

use std::collections::VecDeque;
use std::sync::Mutex;

use axum::extract::ws::Utf8Bytes;
use tokio::sync::broadcast;

use crate::messages::ServerMessage;

/// Events replayed to a client when it connects.
const EVENT_HISTORY: usize = 200;
/// About 10 s of full-rate telemetry. A client further behind than this skips ahead.
const BROADCAST_CAPACITY: usize = 1024;

pub struct Hub {
    tx: broadcast::Sender<Utf8Bytes>,
    cache: Mutex<Cache>,
}

#[derive(Default)]
struct Cache {
    trajectory: Option<Utf8Bytes>,
    params: Option<Utf8Bytes>,
    stand_status: Option<Utf8Bytes>,
    link: Option<Utf8Bytes>,
    events: VecDeque<Utf8Bytes>,
}

impl Hub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            tx,
            cache: Mutex::new(Cache::default()),
        }
    }

    /// Serializes once and hands the same bytes to every client. Never blocks: clients
    /// that cannot keep up lose messages instead of stalling the UDP path.
    pub fn publish(&self, message: &ServerMessage) {
        let json = Utf8Bytes::from(message.to_json());
        {
            let mut cache = self.lock_cache();
            match message {
                ServerMessage::Trajectory(_) => cache.trajectory = Some(json.clone()),
                ServerMessage::Params(_) => cache.params = Some(json.clone()),
                ServerMessage::StandStatus(_) => cache.stand_status = Some(json.clone()),
                ServerMessage::Link(_) => cache.link = Some(json.clone()),
                ServerMessage::Event(_) => {
                    if cache.events.len() == EVENT_HISTORY {
                        cache.events.pop_front();
                    }
                    cache.events.push_back(json.clone());
                }
                _ => {}
            }
        }
        // An error only means no client is connected right now.
        let _ = self.tx.send(json);
    }

    /// Subscribes to live messages and returns the catch-up messages to send first.
    /// Subscribing before taking the snapshot means nothing is missed in between
    /// (a message may be seen twice, which is harmless).
    pub fn join(&self) -> (Vec<Utf8Bytes>, broadcast::Receiver<Utf8Bytes>) {
        let rx = self.tx.subscribe();
        let cache = self.lock_cache();
        let snapshot = cache
            .trajectory
            .iter()
            .chain(&cache.params)
            .chain(&cache.stand_status)
            .chain(&cache.link)
            .chain(&cache.events)
            .cloned()
            .collect();
        (snapshot, rx)
    }

    fn lock_cache(&self) -> std::sync::MutexGuard<'_, Cache> {
        // The cache holds plain data, so it is still usable if a holder panicked.
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gs_protocol::{EventMsg, Severity};

    fn event(i: usize) -> ServerMessage {
        ServerMessage::Event(EventMsg {
            time_s: i as f64,
            severity: Severity::Info,
            text: format!("event {i}"),
        })
    }

    #[test]
    fn new_clients_get_the_latest_stand_status() {
        use gs_protocol::{StandMode, StandStatus};
        let status = |mode| {
            ServerMessage::StandStatus(StandStatus {
                time_s: 0.0,
                mode,
                actuation_link_ok: true,
                loadcell_link_ok: true,
                sequences: vec!["hotfire".into()],
                sequence: None,
            })
        };
        let hub = Hub::new();
        hub.publish(&status(StandMode::Safe));
        hub.publish(&status(StandMode::Armed));
        hub.publish(&event(1));

        let (snapshot, _rx) = hub.join();
        assert_eq!(snapshot.len(), 2);
        assert!(snapshot[0].as_str().contains(r#""type":"stand_status""#));
        assert!(snapshot[0].as_str().contains(r#""mode":"Armed""#));
        assert!(snapshot[1].as_str().contains("event 1"));
    }

    #[test]
    fn new_clients_get_the_last_200_events() {
        let hub = Hub::new();
        for i in 0..250 {
            hub.publish(&event(i));
        }
        hub.publish(&ServerMessage::error("not cached"));

        let (snapshot, _rx) = hub.join();
        assert_eq!(snapshot.len(), EVENT_HISTORY);
        assert!(snapshot[0].as_str().contains("event 50"));
        assert!(snapshot[199].as_str().contains("event 249"));
    }
}
