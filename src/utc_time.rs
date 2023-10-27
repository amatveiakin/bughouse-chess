use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, PrimitiveDateTime};


#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UtcDateTime(PrimitiveDateTime);

impl UtcDateTime {
    pub fn now() -> Self {
        let now_odt = OffsetDateTime::now_utc();
        Self(PrimitiveDateTime::new(now_odt.date(), now_odt.time()))
    }
}

impl From<UtcDateTime> for OffsetDateTime {
    fn from(udt: UtcDateTime) -> Self { udt.0.assume_utc() }
}
