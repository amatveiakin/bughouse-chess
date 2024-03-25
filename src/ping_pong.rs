use std::cmp;
use std::time::Duration;

use instant::Instant;


// TODO: How often should we send pings?
pub const PING_INTERVAL: Duration = Duration::from_millis(500);
pub const OTHER_PARTY_TEMPORARY_LOST_THRESHOLD: Duration = Duration::from_secs(3);
pub const OTHER_PARTY_PERMANENTLY_LOST_THRESHOLD: Duration = Duration::from_secs(60);

// These many ping values will be excluded from statistics after each (re)connection, as they are
// usually outliers. This does not affect ping displayed to the user.
pub const FIRST_PINGS_TO_EXCLUDE: usize = 10;


// Connection monitor for the party that replies to pings with pongs.
#[derive(Debug)]
pub struct PassiveConnectionMonitor {
    latest_incoming: Instant,
}

// Connection monitor for the party that sends pings.
#[derive(Debug)]
pub struct ActiveConnectionMonitor {
    instantiated: Instant,
    latest_turnaround_time: Option<Duration>,
    latest_ping_sent: Option<Instant>,
    latest_ping_answered: bool,
    connected_reset: bool,
    pongs_received_after_reset: usize,
}

#[must_use]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PassiveConnectionStatus {
    // Connection is healty.
    Healthy,

    // The other party hasn't responded for a short period of time.
    // It's ok to close the connection if required, but it's also ok to keep it open in case
    // the other party comes back.
    TemporaryLost,

    // The other party hasn't responded for a long time.
    // They should be considered irrevocably lost.
    PermanentlyLost,
}


#[must_use]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActiveConnectionStatus {
    // No need to do anything.
    Noop,

    // Need to send a ping in order to keep the connection healthy.
    // `ActiveConnectionMonitor` assumes that a ping will be sent when this is returned.
    SendPing,
}

impl PassiveConnectionStatus {
    pub fn is_healthy(self) -> bool { self == PassiveConnectionStatus::Healthy }
}

impl PassiveConnectionMonitor {
    pub fn new(now: Instant) -> Self { PassiveConnectionMonitor { latest_incoming: now } }

    pub fn latest_incoming(&self) -> Instant { self.latest_incoming }

    pub fn register_incoming(&mut self, now: Instant) {
        self.latest_incoming = cmp::max(self.latest_incoming, now);
    }

    pub fn status(&self, now: Instant) -> PassiveConnectionStatus {
        let d = now.saturating_duration_since(self.latest_incoming);
        if d >= OTHER_PARTY_PERMANENTLY_LOST_THRESHOLD {
            PassiveConnectionStatus::PermanentlyLost
        } else if d >= OTHER_PARTY_TEMPORARY_LOST_THRESHOLD {
            PassiveConnectionStatus::TemporaryLost
        } else {
            PassiveConnectionStatus::Healthy
        }
    }
}

impl ActiveConnectionMonitor {
    pub fn new(now: Instant) -> Self {
        ActiveConnectionMonitor {
            instantiated: now,
            latest_turnaround_time: None,
            latest_ping_sent: None,
            latest_ping_answered: true,
            connected_reset: false,
            pongs_received_after_reset: 0,
        }
    }

    // Should be called after WebSocket connection is recreated.
    pub fn reset(&mut self) {
        self.connected_reset = true;
        self.pongs_received_after_reset = 0;
    }

    pub fn update(&mut self, now: Instant) -> ActiveConnectionStatus {
        if !self.connected_reset {
            if !self.latest_ping_answered {
                return ActiveConnectionStatus::Noop;
            }
            if let Some(latest_ping_sent) = self.latest_ping_sent {
                if now.duration_since(latest_ping_sent) < PING_INTERVAL {
                    return ActiveConnectionStatus::Noop;
                }
            }
        }
        self.connected_reset = false;
        self.latest_ping_sent = Some(now);
        self.latest_ping_answered = false;
        ActiveConnectionStatus::SendPing
    }

    pub fn register_pong(&mut self, now: Instant) -> Option<Duration> {
        // Normally this should hold, be we could receive multiple replies after a (re)connection.
        //   assert!(!self.latest_ping_answered);

        self.latest_ping_answered = true;
        let d = now.duration_since(self.latest_ping_sent.unwrap());
        self.latest_turnaround_time = Some(d);
        self.pongs_received_after_reset += 1;
        (self.pongs_received_after_reset >= FIRST_PINGS_TO_EXCLUDE).then_some(d)
    }

    pub fn current_turnaround_time(&self, now: Instant) -> Duration {
        let Some(mut t) = self.latest_turnaround_time else {
            return now.duration_since(self.instantiated);
        };
        if !self.latest_ping_answered {
            let latest_ping_sent = self.latest_ping_sent.unwrap();
            t = cmp::max(t, now.duration_since(latest_ping_sent))
        }
        t
    }
}
