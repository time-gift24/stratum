//! Configuration for the AgentRuntime-scoped NATS tail transport.

use std::time::Duration;

use async_nats::jetstream::stream::{self, DiscardPolicy, RetentionPolicy, StorageType};

use super::{AgentRuntimeTailError, subject};

/// Configuration for the AgentRuntime-scoped NATS tail transport.
///
/// All three retention limits are required and finite: the tail is a short,
/// lossy observation channel with discard-old limits retention, never a
/// durable history.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AgentRuntimeTailConfig {
    /// NATS server URL.
    pub url: String,
    /// JetStream stream name backing all AgentRuntime tails.
    pub stream_name: String,
    /// Subject prefix before `runtime.<agent_runtime_id>`.
    pub subject_prefix: String,
    /// Number of stream replicas.
    pub replicas: usize,
    /// Maximum retained frame age.
    pub max_age: Duration,
    /// Maximum retained stream size in bytes.
    pub max_bytes: i64,
    /// Maximum retained frame count.
    pub max_messages: i64,
}

impl Default for AgentRuntimeTailConfig {
    fn default() -> Self {
        Self {
            url: "nats://localhost:4222".to_owned(),
            stream_name: "AGENT_RUNTIME_TAIL".to_owned(),
            subject_prefix: "events.agent".to_owned(),
            replicas: 1,
            max_age: Duration::from_secs(60 * 60),
            max_bytes: 67_108_864,
            max_messages: 100_000,
        }
    }
}

impl AgentRuntimeTailConfig {
    pub(crate) fn validate(&self) -> Result<(), AgentRuntimeTailError> {
        let reason = if self.stream_name.is_empty() {
            Some("stream_name must not be empty")
        } else if !valid_subject_prefix(&self.subject_prefix) {
            Some("subject_prefix must be non-empty dot-separated tokens without wildcards")
        } else if self.max_age.is_zero() {
            Some("max_age must be greater than zero")
        } else if self.max_bytes <= 0 {
            Some("max_bytes must be greater than zero")
        } else if self.max_messages <= 0 {
            Some("max_messages must be greater than zero")
        } else if !(1..=5).contains(&self.replicas) {
            Some("replicas must be between 1 and 5")
        } else {
            None
        };

        reason.map_or(Ok(()), |reason| {
            Err(AgentRuntimeTailError::InvalidConfig { reason })
        })
    }

    pub(crate) fn stream_config(&self) -> stream::Config {
        stream::Config {
            name: self.stream_name.clone(),
            subjects: vec![subject::stream_catch_subject(&self.subject_prefix)],
            storage: StorageType::File,
            retention: RetentionPolicy::Limits,
            discard: DiscardPolicy::Old,
            max_age: self.max_age,
            max_bytes: self.max_bytes,
            max_messages: self.max_messages,
            num_replicas: self.replicas,
            ..Default::default()
        }
    }
}

fn valid_subject_prefix(prefix: &str) -> bool {
    !prefix.is_empty()
        && prefix
            .split('.')
            .all(|token| !token.is_empty() && !token.contains(['*', '>']))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_finite_discard_old_retention_limits() {
        let config = AgentRuntimeTailConfig::default();
        config.validate().expect("default config is valid");

        let stream_config = config.stream_config();
        assert_eq!(stream_config.storage, StorageType::File);
        assert_eq!(stream_config.retention, RetentionPolicy::Limits);
        assert_eq!(stream_config.discard, DiscardPolicy::Old);
        assert_eq!(stream_config.max_age, config.max_age);
        assert!(stream_config.max_age > Duration::ZERO);
        assert_eq!(stream_config.max_bytes, config.max_bytes);
        assert!(stream_config.max_bytes > 0);
        assert_eq!(stream_config.max_messages, config.max_messages);
        assert!(stream_config.max_messages > 0);
        assert_eq!(stream_config.num_replicas, config.replicas);
        assert_eq!(
            stream_config.subjects,
            vec![subject::stream_catch_subject(&config.subject_prefix)]
        );
    }

    #[test]
    fn config_rejects_non_finite_retention_limits() {
        let invalid_configs = [
            AgentRuntimeTailConfig {
                max_age: Duration::ZERO,
                ..Default::default()
            },
            AgentRuntimeTailConfig {
                max_bytes: 0,
                ..Default::default()
            },
            AgentRuntimeTailConfig {
                max_bytes: -1,
                ..Default::default()
            },
            AgentRuntimeTailConfig {
                max_messages: 0,
                ..Default::default()
            },
            AgentRuntimeTailConfig {
                max_messages: -1,
                ..Default::default()
            },
        ];

        for config in invalid_configs {
            assert!(matches!(
                config.validate(),
                Err(AgentRuntimeTailError::InvalidConfig { .. })
            ));
        }
    }

    #[test]
    fn config_rejects_invalid_identity_fields() {
        let invalid_configs = [
            AgentRuntimeTailConfig {
                stream_name: String::new(),
                ..Default::default()
            },
            AgentRuntimeTailConfig {
                subject_prefix: String::new(),
                ..Default::default()
            },
            AgentRuntimeTailConfig {
                subject_prefix: "events..agent".to_owned(),
                ..Default::default()
            },
            AgentRuntimeTailConfig {
                subject_prefix: "events.agent.>".to_owned(),
                ..Default::default()
            },
            AgentRuntimeTailConfig {
                replicas: 0,
                ..Default::default()
            },
            AgentRuntimeTailConfig {
                replicas: 6,
                ..Default::default()
            },
        ];

        for config in invalid_configs {
            assert!(matches!(
                config.validate(),
                Err(AgentRuntimeTailError::InvalidConfig { .. })
            ));
        }
    }
}
