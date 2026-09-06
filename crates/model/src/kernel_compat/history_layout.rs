//! Version-owned read-only history schema. Product storage owns only associations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodexHistoryLayout {
    pub database_name: &'static str,
    pub thread_exists_query: &'static str,
}

impl CodexHistoryLayout {
    /// Audited for the historical slim revision and official 28327355.
    pub(crate) const STATE_V5: Self = Self {
        database_name: "state_5.sqlite",
        thread_exists_query: "SELECT EXISTS(SELECT 1 FROM threads WHERE id = ?1)",
    };

    pub fn storage_probe(self) -> (&'static str, &'static str) {
        (self.database_name, self.thread_exists_query)
    }

    /// The legacy memory viewer is deliberately unavailable for candidate kernels.
    pub fn legacy_memory() -> Self {
        Self::STATE_V5
    }
}
