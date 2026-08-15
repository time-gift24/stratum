//! Atomically swappable provider catalog for future Turn starts.
//!
//! A Turn takes one [`Arc`] snapshot before constructing its kernel. Replacing
//! the catalog therefore affects only later starts and never changes a
//! provider already pinned by an in-flight Turn.

use std::sync::{Arc, RwLock};

use stratum_llm::LlmProviderManager;

/// Process-local provider catalog with short, non-async critical sections.
pub(crate) struct ProviderCatalog {
    current: RwLock<Arc<LlmProviderManager>>,
}

impl ProviderCatalog {
    /// Creates a catalog from its first complete provider registry.
    pub(crate) fn new(providers: LlmProviderManager) -> Self {
        Self {
            current: RwLock::new(Arc::new(providers)),
        }
    }

    /// Clones the immutable registry used by one request or Turn start.
    #[must_use]
    pub(crate) fn snapshot(&self) -> Arc<LlmProviderManager> {
        self.current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Publishes a complete replacement registry for future reads.
    pub(crate) fn replace(&self, providers: LlmProviderManager) {
        *self
            .current
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Arc::new(providers);
    }
}
