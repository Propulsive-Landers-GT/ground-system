//! Downlink health: packet counts, loss from `FlightTelemetry.seq` gaps, flight packet rate.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::messages::LinkStatus;

/// `connected` means a packet arrived within this window.
const CONNECTED_WINDOW: Duration = Duration::from_secs(1);
const RATE_WINDOW: Duration = Duration::from_secs(1);

/// A sequence number this far behind the newest one is a late (reordered) datagram;
/// anything further back means the vehicle restarted its counter.
const REORDER_WINDOW: u32 = 50;

#[derive(Debug, Default)]
pub struct LinkStats {
    packets_rx: u64,
    packets_lost: u64,
    last_rx: Option<Instant>,
    last_flight_seq: Option<u32>,
    /// Arrival times of flight packets within the last [`RATE_WINDOW`].
    flight_arrivals: VecDeque<Instant>,
}

impl LinkStats {
    /// Call for every valid downlink packet.
    pub fn on_packet(&mut self, now: Instant) {
        self.packets_rx += 1;
        self.last_rx = Some(now);
    }

    /// Call additionally for every flight packet.
    pub fn on_flight_seq(&mut self, seq: u32, now: Instant) {
        self.flight_arrivals.push_back(now);
        self.trim_arrivals(now);

        let Some(last) = self.last_flight_seq else {
            self.last_flight_seq = Some(seq);
            return;
        };
        if seq > last {
            self.packets_lost += u64::from(seq - last - 1);
            self.last_flight_seq = Some(seq);
        } else if last - seq > REORDER_WINDOW {
            // Vehicle restarted: start counting from its new sequence.
            self.last_flight_seq = Some(seq);
        }
        // Otherwise a duplicate or late packet. It was already counted as lost when the
        // gap was seen; leaving that in place keeps the counter monotonic.
    }

    pub fn connected(&self, now: Instant) -> bool {
        self.last_rx
            .is_some_and(|t| now.duration_since(t) <= CONNECTED_WINDOW)
    }

    pub fn status(
        &mut self,
        now: Instant,
        vehicle_addr: String,
        recording: Option<String>,
    ) -> LinkStatus {
        self.trim_arrivals(now);
        LinkStatus {
            vehicle_addr,
            connected: self.connected(now),
            last_rx_age_s: self.last_rx.map(|t| now.duration_since(t).as_secs_f64()),
            packets_rx: self.packets_rx,
            packets_lost: self.packets_lost,
            rate_hz: self.flight_arrivals.len() as f64 / RATE_WINDOW.as_secs_f64(),
            recording,
        }
    }

    fn trim_arrivals(&mut self, now: Instant) {
        while self
            .flight_arrivals
            .front()
            .is_some_and(|&t| now.duration_since(t) > RATE_WINDOW)
        {
            self.flight_arrivals.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(stats: &mut LinkStats, now: Instant, seqs: &[u32]) {
        for &seq in seqs {
            stats.on_packet(now);
            stats.on_flight_seq(seq, now);
        }
    }

    fn lost(stats: &mut LinkStats, now: Instant) -> u64 {
        stats.status(now, String::new(), None).packets_lost
    }

    #[test]
    fn counts_gaps() {
        let now = Instant::now();
        let mut stats = LinkStats::default();
        feed(&mut stats, now, &[100, 101, 102, 105, 106, 110]);
        assert_eq!(lost(&mut stats, now), 2 + 3);
    }

    #[test]
    fn first_packet_is_not_a_gap() {
        let now = Instant::now();
        let mut stats = LinkStats::default();
        feed(&mut stats, now, &[5000]);
        assert_eq!(lost(&mut stats, now), 0);
    }

    #[test]
    fn vehicle_restart_resets_instead_of_counting_loss() {
        let now = Instant::now();
        let mut stats = LinkStats::default();
        feed(&mut stats, now, &[9000, 9001, 0, 1, 2, 4]);
        assert_eq!(lost(&mut stats, now), 1);
    }

    #[test]
    fn late_and_duplicate_packets_are_ignored() {
        let now = Instant::now();
        let mut stats = LinkStats::default();
        feed(&mut stats, now, &[10, 12, 11, 12, 13]);
        assert_eq!(lost(&mut stats, now), 1);
    }

    #[test]
    fn rate_and_connected_use_a_one_second_window() {
        let start = Instant::now();
        let mut stats = LinkStats::default();
        for i in 0..100u32 {
            let now = start + Duration::from_millis(20 * u64::from(i));
            stats.on_packet(now);
            stats.on_flight_seq(i, now);
        }
        let end = start + Duration::from_millis(1990);
        let status = stats.status(end, "v".into(), None);
        assert!(status.connected);
        assert!((status.rate_hz - 50.0).abs() <= 1.0, "{}", status.rate_hz);
        assert_eq!(status.packets_rx, 100);

        let later = end + Duration::from_secs(3);
        let status = stats.status(later, "v".into(), None);
        assert!(!status.connected);
        assert_eq!(status.rate_hz, 0.0);
        assert!(status.last_rx_age_s.unwrap() > 2.9);
    }
}
