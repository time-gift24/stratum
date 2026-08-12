//! Opaque cursor into one AgentRuntime's retained NATS tail.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use stratum_core::AgentRuntimeId;

use super::AgentRuntimeTailCursorParseError;

/// Opaque position in one AgentRuntime's short retained NATS tail.
///
/// A cursor binds an AgentRuntime, a JetStream stream creation generation, and a
/// stream sequence with short, finite retention. It is not a durable event sequence: it must never be compared with
/// `event_seq`/telemetry sequences, persisted as business state, or interpreted
/// beyond choosing a tail position. Expiry is reported as the typed
/// [`super::AgentRuntimeTailError::CursorExpired`] error.
///
/// The versioned string form is the opaque SSE `id` transport encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AgentRuntimeTailCursor {
    agent_runtime_id: AgentRuntimeId,
    stream_generation: i128,
    sequence: u64,
}

impl AgentRuntimeTailCursor {
    pub(crate) const fn from_transport(
        agent_runtime_id: AgentRuntimeId,
        stream_generation: i128,
        sequence: u64,
    ) -> Self {
        Self {
            agent_runtime_id,
            stream_generation,
            sequence,
        }
    }

    pub(crate) const fn transport_sequence(self) -> u64 {
        self.sequence
    }

    pub(crate) fn belongs_to(
        self,
        agent_runtime_id: AgentRuntimeId,
        stream_generation: i128,
    ) -> bool {
        self.agent_runtime_id == agent_runtime_id && self.stream_generation == stream_generation
    }
}

impl fmt::Display for AgentRuntimeTailCursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "v1.{}.{}.{}",
            self.agent_runtime_id, self.stream_generation, self.sequence
        )
    }
}

impl FromStr for AgentRuntimeTailCursor {
    type Err = AgentRuntimeTailCursorParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.split('.');
        let version = parts.next();
        let agent_runtime_id = parts
            .next()
            .and_then(|part| part.parse::<AgentRuntimeId>().ok());
        let stream_generation = parts.next().and_then(|part| part.parse::<i128>().ok());
        let sequence = parts.next().and_then(|part| part.parse::<u64>().ok());
        if version != Some("v1") || parts.next().is_some() {
            return Err(AgentRuntimeTailCursorParseError);
        }
        match (agent_runtime_id, stream_generation, sequence) {
            (Some(agent_runtime_id), Some(stream_generation), Some(sequence)) => Ok(Self {
                agent_runtime_id,
                stream_generation,
                sequence,
            }),
            _ => Err(AgentRuntimeTailCursorParseError),
        }
    }
}

impl Serialize for AgentRuntimeTailCursor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for AgentRuntimeTailCursor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_string_roundtrip_uses_versioned_runtime_scoped_encoding() {
        let agent_runtime_id = AgentRuntimeId::new();
        let cursor = AgentRuntimeTailCursor::from_transport(agent_runtime_id, 1234, 42);

        let encoded = format!("v1.{agent_runtime_id}.1234.42");
        assert_eq!(cursor.to_string(), encoded);
        assert_eq!(encoded.parse::<AgentRuntimeTailCursor>(), Ok(cursor));
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
                input.parse::<AgentRuntimeTailCursor>(),
                Err(AgentRuntimeTailCursorParseError)
            ));
        }
    }

    #[test]
    fn cursor_binding_rejects_another_runtime_or_stream_generation() {
        let agent_runtime_id = AgentRuntimeId::new();
        let cursor = AgentRuntimeTailCursor::from_transport(agent_runtime_id, 1234, 42);

        assert!(cursor.belongs_to(agent_runtime_id, 1234));
        assert!(!cursor.belongs_to(AgentRuntimeId::new(), 1234));
        assert!(!cursor.belongs_to(agent_runtime_id, 1235));
    }

    #[test]
    fn cursor_serde_uses_string_representation() {
        let cursor = AgentRuntimeTailCursor::from_transport(AgentRuntimeId::new(), 99, 7);

        let encoded = serde_json::to_string(&cursor).expect("cursor serializes");
        assert_eq!(encoded, format!("\"{cursor}\""));
        let decoded: AgentRuntimeTailCursor =
            serde_json::from_str(&encoded).expect("cursor deserializes");
        assert_eq!(decoded, cursor);
        assert!(serde_json::from_str::<AgentRuntimeTailCursor>("7").is_err());
    }
}
