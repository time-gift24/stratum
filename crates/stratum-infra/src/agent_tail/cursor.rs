//! Opaque cursor into one agent's retained NATS tail.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::TailCursorParseError;

/// Opaque position in one agent's short retained NATS tail.
///
/// A cursor wraps a JetStream stream sequence with short, finite retention. It
/// is not a durable event sequence: it must never be compared with
/// `event_seq`/telemetry sequences, persisted as business state, or interpreted
/// beyond choosing a tail position. Expiry is reported as the typed
/// [`super::AgentTailError::CursorExpired`] error.
///
/// The string form (decimal digits) is the SSE `id` transport encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TailCursor(u64);

impl TailCursor {
    pub(crate) const fn from_transport_sequence(sequence: u64) -> Self {
        Self(sequence)
    }

    pub(crate) const fn transport_sequence(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TailCursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for TailCursor {
    type Err = TailCursorParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<u64>()
            .map(Self)
            .map_err(|_| TailCursorParseError)
    }
}

impl Serialize for TailCursor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for TailCursor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_string_roundtrip_uses_decimal_encoding() {
        let cursor = TailCursor::from_transport_sequence(42);

        assert_eq!(cursor.to_string(), "42");
        assert_eq!("42".parse::<TailCursor>(), Ok(cursor));
    }

    #[test]
    fn cursor_parse_rejects_non_decimal_input() {
        for input in ["", "-1", "1.5", "abc", "18446744073709551616"] {
            assert!(matches!(
                input.parse::<TailCursor>(),
                Err(TailCursorParseError)
            ));
        }
    }

    #[test]
    fn cursor_serde_uses_string_representation() {
        let cursor = TailCursor::from_transport_sequence(7);

        let encoded = serde_json::to_string(&cursor).expect("cursor serializes");
        assert_eq!(encoded, "\"7\"");
        let decoded: TailCursor = serde_json::from_str(&encoded).expect("cursor deserializes");
        assert_eq!(decoded, cursor);
        assert!(serde_json::from_str::<TailCursor>("7").is_err());
    }
}
