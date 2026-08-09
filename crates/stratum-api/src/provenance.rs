//! Committed-context provenance lineage.
//!
//! The per-turn sink mirrors the kernel's committed context: every entry is
//! the origin AgentRuntime-wide `event_seq` of the `MessageAppended` the context
//! message came from, or `None` for synthetic summary markers (which have no
//! durable message origin). When the kernel appends `TranscriptCompacted`,
//! the lineage resolves the first retained message's durable pointer from the
//! kernel's `upto` coordinate; failure to resolve fails the append closed.

/// Origin sequence of each message in the committed context, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ContextLineage {
    origins: Vec<Option<u64>>,
}

impl ContextLineage {
    /// Builds a lineage from ordered origin sequences.
    #[must_use]
    pub(crate) fn from_origins(origins: Vec<Option<u64>>) -> Self {
        Self { origins }
    }

    /// Records one committed message by its assigned sequence.
    pub(crate) fn record_message(&mut self, event_seq: u64) {
        self.origins.push(Some(event_seq));
    }

    /// Resolves the durable pointer of the first message retained after a
    /// compaction cut of `[0, upto)`.
    ///
    /// The kernel's cut invariant guarantees at least one retained message, so
    /// `upto` must be inside the lineage and address a real
    /// `MessageAppended`; a synthetic marker or an out-of-range cut fails the
    /// resolution (the store would also fail the append closed).
    #[must_use]
    pub(crate) fn retained_from(&self, upto: u64) -> Option<u64> {
        self.origins
            .get(usize::try_from(upto).ok()?)
            .copied()
            .flatten()
    }

    /// Applies one committed compaction cut: the replaced prefix becomes one
    /// synthetic marker placeholder and the retained suffix keeps its origins.
    pub(crate) fn apply_compaction(&mut self, upto: u64) {
        let upto = usize::try_from(upto).unwrap_or(usize::MAX);
        if upto > self.origins.len() {
            // The store already rejected this append; keep the lineage
            // untouched so later appends stay consistent.
            return;
        }
        self.origins.splice(..upto, std::iter::once(None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_pointer_tracks_message_origins_across_compactions() {
        // Baseline: marker + two retained messages from history.
        let mut lineage = ContextLineage::from_origins(vec![None, Some(5), Some(6)]);
        // The run commits three more messages.
        lineage.record_message(8);
        lineage.record_message(9);
        lineage.record_message(10);

        // Cut [0, 2): the first retained message is the one from seq 6.
        assert_eq!(lineage.retained_from(2), Some(6));
        lineage.apply_compaction(2);
        assert_eq!(
            lineage,
            ContextLineage::from_origins(vec![None, Some(6), Some(8), Some(9), Some(10)])
        );

        // A second cut of [0, 3) retains the seq-9 message first.
        assert_eq!(lineage.retained_from(3), Some(9));
        lineage.apply_compaction(3);
        assert_eq!(
            lineage,
            ContextLineage::from_origins(vec![None, Some(9), Some(10)])
        );
    }

    #[test]
    fn unresolved_pointers_fail_closed() {
        let lineage = ContextLineage::from_origins(vec![None, Some(5)]);
        // A cut whose first retained entry is the synthetic marker.
        assert_eq!(lineage.retained_from(0), None);
        // A cut past the context end.
        assert_eq!(lineage.retained_from(2), None);
        assert_eq!(lineage.retained_from(u64::MAX), None);
    }
}
