//! Centralized NATS subject naming for Agent tails.
//!
//! All tail subjects live under `{prefix}.agent.`; the stream catches
//! `{prefix}.agent.>` and each agent publishes and subscribes exactly one
//! subject. No subject string literal may appear outside this module.

use stratum_core::AgentId;

/// Stream-level catch subject covering every agent tail.
pub(crate) fn stream_catch_subject(prefix: &str) -> String {
    format!("{prefix}.agent.>")
}

/// Subject carrying exactly one agent's tail frames.
pub(crate) fn agent_subject(prefix: &str, agent_id: &AgentId) -> String {
    format!("{prefix}.agent.{agent_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_subject_is_scoped_by_agent_id() {
        let agent_id = AgentId::new();

        assert_eq!(
            agent_subject("events.agent", &agent_id),
            format!("events.agent.agent.{agent_id}")
        );
    }

    #[test]
    fn stream_catch_subject_covers_all_agent_subjects() {
        let agent_id = AgentId::new();
        let catch = stream_catch_subject("events.agent");
        let agent = agent_subject("events.agent", &agent_id);

        assert_eq!(catch, "events.agent.agent.>");
        let covered = agent
            .strip_prefix(catch.strip_suffix('>').expect("catch suffix"))
            .expect("agent subject is covered by the catch subject");
        assert!(!covered.is_empty() && !covered.contains(['*', '>']));
    }
}
