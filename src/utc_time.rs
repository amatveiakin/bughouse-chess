use serde::{Deserialize, Serialize};
use time::macros::{datetime, offset};
use time::{OffsetDateTime, PrimitiveDateTime};


#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UtcDateTime(PrimitiveDateTime);

impl UtcDateTime {
    pub const UNIX_EPOCH: Self = Self(datetime!(1970-01-01 0:00));
    pub fn now() -> Self {
        let now_odt = OffsetDateTime::now_utc();
        Self(PrimitiveDateTime::new(now_odt.date(), now_odt.time()))
    }
}

impl From<PrimitiveDateTime> for UtcDateTime {
    fn from(pdt: PrimitiveDateTime) -> Self { Self(pdt) }
}
impl From<OffsetDateTime> for UtcDateTime {
    fn from(odt: OffsetDateTime) -> Self {
        let utc = odt.to_offset(offset!(UTC));
        Self::from(PrimitiveDateTime::new(utc.date(), utc.time()))
    }
}

impl From<UtcDateTime> for OffsetDateTime {
    fn from(udt: UtcDateTime) -> Self { udt.0.assume_utc() }
}
