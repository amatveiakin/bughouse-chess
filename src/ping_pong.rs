use std::cmp;
use std::time::Duration;

use instant::Instant;


// TODO: How often should we send pings?
pub const PING_INTERVAL: Duration = Duration::from_millis(500);
pub const OTHER_PARTY_TEMPORARY_LOST_THRESHOLD: Duration = Duration::from_secs(3);
pub const OTHER_PARTY_PERMANENTLY_LOST_THRESHOLD: Duration = Duration::from_secs(60);


// Connection monitor for the party that replies to pings with pongs.
pub struct PassiveConnectionMonitor {
    latest_incoming: Instant,
}

// Connection monitor for the party that sends pings.
pub struct ActiveConnectionMonitor {
    latest_turnaround_time: Option<Duration>,
    latest_ping_sent: Option<Instant>,
    latest_ping_answered: bool,
    first_ping: bool,
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

    pub fn status(&mut self, now: Instant) -> PassiveConnectionStatus {
        if now.saturating_duration_since(self.latest_incoming)
            >= OTHER_PARTY_PERMANENTLY_LOST_THRESHOLD
        {
            PassiveConnectionStatus::PermanentlyLost
        } else if now.saturating_duration_since(self.latest_incoming)
            >= OTHER_PARTY_TEMPORARY_LOST_THRESHOLD
        {
            PassiveConnectionStatus::TemporaryLost
        } else {
            PassiveConnectionStatus::Healthy
        }
    }
}

impl ActiveConnectionMonitor {
    pub fn new() -> Self {
        ActiveConnectionMonitor {
            latest_turnaround_time: None,
            latest_ping_sent: None,
            latest_ping_answered: true,
            first_ping: true,
        }
    }

    pub fn update(&mut self, now: Instant) -> ActiveConnectionStatus {
        if !self.latest_ping_answered {
            return ActiveConnectionStatus::Noop;
        }
        if let Some(latest_ping_sent) = self.latest_ping_sent {
            if now.duration_since(latest_ping_sent) < PING_INTERVAL {
                return ActiveConnectionStatus::Noop;
            }
        };
        self.latest_ping_sent = Some(now);
        self.latest_ping_answered = false;
        ActiveConnectionStatus::SendPing
    }

    pub fn register_pong(&mut self, now: Instant) -> Option<Duration> {
        assert!(!self.latest_ping_answered);
        self.latest_ping_answered = true;
        if self.first_ping {
            self.first_ping = false;
            // Ignore the first ping value: it is usually an outlier.
            // Improvement potential. Consider if more pings should be ignored in the beginning.
            None
        } else {
            let d = now.duration_since(self.latest_ping_sent.unwrap());
            self.latest_turnaround_time = Some(d);
            Some(d)
        }
    }

    pub fn current_turnaround_time(&self, now: Instant) -> Option<Duration> {
        let mut t = self.latest_turnaround_time?;
        if !self.latest_ping_answered {
            let latest_ping_sent = self.latest_ping_sent.unwrap();
            t = cmp::max(t, now.duration_since(latest_ping_sent))
        }
        Some(t)
    }
}

impl Default for ActiveConnectionMonitor {
    fn default() -> Self { Self::new() }
}
