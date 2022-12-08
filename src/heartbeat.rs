use std::cmp;
use std::time::Duration;

use instant::Instant;


pub const OUTGOING_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
pub const OTHER_PARTY_TEMPORARY_LOST_THRESHOLD: Duration = Duration::from_secs(3);
pub const OTHER_PARTY_PERMANENTLY_LOST_THRESHOLD: Duration = Duration::from_secs(60);

#[must_use]
pub enum HeartbeatOutcome {
    // Connection is healty.
    // Action. None required.
    AllGood,

    // This party hasn't sent messages for a long time.
    // Action. Send a heartbeat message. Note that when the heartbeat is sent, it sould be
    // recorded with `register_outgoing`, like any other event.
    SendBeat,

    // The other party hasn't responded for a short period of time.
    // Action. Report connection problems to the user. Keep the network channel open in case
    // the other party comes back.
    OtherPartyTemporatyLost,

    // The other party hasn't responded for a long time.
    // Action. Consider the other party irrevocably lost and act accordingly.
    OtherPartyPermanentlyLost,
}

pub struct Heart {
    latest_incoming: Instant,
    latest_outgoing: Instant,
    healthy: bool,
}

impl Heart {
    pub fn new(now: Instant) -> Self {
        Heart {
            latest_incoming: now,
            latest_outgoing: now,
            healthy: true,
        }
    }

    pub fn healthy(&self) -> bool { self.healthy }

    pub fn register_incoming(&mut self, now: Instant) {
        self.latest_incoming = cmp::max(self.latest_incoming, now);
    }
    pub fn register_outgoing(&mut self, now: Instant) {
        self.latest_outgoing = cmp::max(self.latest_outgoing, now);
    }

    pub fn beat(&mut self, now: Instant) -> HeartbeatOutcome {
        if now.saturating_duration_since(self.latest_incoming) > OTHER_PARTY_PERMANENTLY_LOST_THRESHOLD {
            self.healthy = false;
            HeartbeatOutcome::OtherPartyPermanentlyLost
        } else if now.saturating_duration_since(self.latest_incoming) > OTHER_PARTY_TEMPORARY_LOST_THRESHOLD {
            self.healthy = false;
            HeartbeatOutcome::OtherPartyTemporatyLost
        } else if now.saturating_duration_since(self.latest_outgoing) > OUTGOING_HEARTBEAT_INTERVAL {
            self.healthy = true;
            HeartbeatOutcome::SendBeat
        } else {
            self.healthy = true;
            HeartbeatOutcome::AllGood
        }
    }
}
