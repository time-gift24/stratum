//! Opaque cursor into one agent's retained NATS tail.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use stratum_core::AgentId;

use super::TailCursorParseError;

/// Opaque position in one agent's short retained NATS tail.
///
/// A cursor binds an Agent, a JetStream stream creation generation, and a
/// stream sequence with short, finite retention. It is not a durable event sequence: it must never be compared with
/// `event_seq`/telemetry sequences, persisted as business state, or interpreted
/// beyond choosing a tail position. Expiry is reported as the typed
/// [`super::AgentTailError::CursorExpired`] error.
///
/// The versioned string form is the opaque SSE `id` transport encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TailCursor {
    agent_id: AgentId,
    stream_generation: i128,
    sequence: u64,
}

impl TailCursor {
    pub(crate) const fn from_transport(
        agent_id: AgentId,
        stream_generation: i128,
        sequence: u64,
    ) -> Self {
        Self {
            agent_id,
            stream_generation,
            sequence,
        }
    }

    pub(crate) const fn transport_sequence(self) -> u64 {
        self.sequence
    }

    pub(crate) fn belongs_to(self, agent_id: AgentId, stream_generation: i128) -> bool {
        self.agent_id == agent_id && self.stream_generation == stream_generation
    }
}

impl fmt::Display for TailCursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "v1.{}.{}.{}",
            self.agent_id, self.stream_generation, self.sequence
        )
    }
}

impl FromStr for TailCursor {
    type Err = TailCursorParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.split('.');
        let version = parts.next();
        let agent_id = parts.next().and_then(|part| part.parse::<AgentId>().ok());
        let stream_generation = parts.next().and_then(|part| part.parse::<i128>().ok());
        let sequence = parts.next().and_then(|part| part.parse::<u64>().ok());
        if version != Some("v1") || parts.next().is_some() {
            return Err(TailCursorParseError);
        }
        match (agent_id, stream_generation, sequence) {
            (Some(agent_id), Some(stream_generation), Some(sequence)) => Ok(Self {
                agent_id,
                stream_generation,
                sequence,
            }),
            _ => Err(TailCursorParseError),
        }
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
    fn cursor_string_roundtrip_uses_versioned_agent_scoped_encoding() {
        let agent_id = AgentId::new();
        let cursor = TailCursor::from_transport(agent_id, 1234, 42);

        let encoded = format!("v1.{agent_id}.1234.42");
        assert_eq!(cursor.to_string(), encoded);
        assert_eq!(encoded.parse::<TailCursor>(), Ok(cursor));
    }

    #[test]
    fn cursor_parse_rejects_malformed_or_legacy_input() {
        for input in [
            "",
            "42",
            "v2.00000000-0000-0000-0000-000000000000.1.2",
            "v1.not-an-agent.1.2",
            "v1.00000000-0000-0000-0000-000000000000.x.2",
            "v1.00000000-0000-0000-0000-000000000000.1.2.extra",
        ] {
            assert!(matches!(
                input.parse::<TailCursor>(),
                Err(TailCursorParseError)
            ));
        }
    }

    #[test]
    fn cursor_binding_rejects_another_agent_or_stream_generation() {
        let agent_id = AgentId::new();
        let cursor = TailCursor::from_transport(agent_id, 1234, 42);

        assert!(cursor.belongs_to(agent_id, 1234));
        assert!(!cursor.belongs_to(AgentId::new(), 1234));
        assert!(!cursor.belongs_to(agent_id, 1235));
    }

    #[test]
    fn cursor_serde_uses_string_representation() {
        let cursor = TailCursor::from_transport(AgentId::new(), 99, 7);

        let encoded = serde_json::to_string(&cursor).expect("cursor serializes");
        assert_eq!(encoded, format!("\"{cursor}\""));
        let decoded: TailCursor = serde_json::from_str(&encoded).expect("cursor deserializes");
        assert_eq!(decoded, cursor);
        assert!(serde_json::from_str::<TailCursor>("7").is_err());
    }
}
