//! Centralized NATS subject naming for AgentRuntime tails.
//!
//! All tail subjects live under `{prefix}.runtime.`; the stream catches
//! `{prefix}.runtime.>` and each AgentRuntime publishes and subscribes exactly one
//! subject. No subject string literal may appear outside this module.

use stratum_core::AgentRuntimeId;

/// Stream-level catch subject covering every AgentRuntime tail.
pub(crate) fn stream_catch_subject(prefix: &str) -> String {
    format!("{prefix}.runtime.>")
}

/// Subject carrying exactly one AgentRuntime's tail frames.
pub(crate) fn runtime_subject(prefix: &str, agent_runtime_id: &AgentRuntimeId) -> String {
    format!("{prefix}.runtime.{agent_runtime_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_subject_is_scoped_by_agent_runtime_id() {
        let agent_runtime_id = AgentRuntimeId::new();

        assert_eq!(
            runtime_subject("events.agent", &agent_runtime_id),
            format!("events.agent.runtime.{agent_runtime_id}")
        );
    }

    #[test]
    fn stream_catch_subject_covers_all_runtime_subjects() {
        let agent_runtime_id = AgentRuntimeId::new();
        let catch = stream_catch_subject("events.agent");
        let runtime = runtime_subject("events.agent", &agent_runtime_id);

        assert_eq!(catch, "events.agent.runtime.>");
        let covered = runtime
            .strip_prefix(catch.strip_suffix('>').expect("catch suffix"))
            .expect("runtime subject is covered by the catch subject");
        assert!(!covered.is_empty() && !covered.contains(['*', '>']));
    }
}
