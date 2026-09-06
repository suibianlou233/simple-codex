use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// A concrete local authority that a tool may require.
///
/// These values describe capabilities only. Granting, approving, and enforcing
/// them belongs to the action policy and sandbox layers above this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    ReadWorkspace,
    WriteWorkspace,
    SpawnProcess,
    AccessNetwork,
    WriteOutsideWorkspace,
    ControlSystem,
}

impl Capability {
    /// Whether exercising this capability may change local or external state.
    #[must_use]
    pub const fn is_effectful(self) -> bool {
        !matches!(self, Self::ReadWorkspace)
    }
}

/// A deterministic set of capabilities used by tool metadata and action intents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilitySet(BTreeSet<Capability>);

impl CapabilitySet {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn contains(&self, capability: Capability) -> bool {
        self.0.contains(&capability)
    }

    pub fn insert(&mut self, capability: Capability) -> bool {
        self.0.insert(capability)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_subset(&self, other: &Self) -> bool {
        self.0.is_subset(&other.0)
    }

    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.0.iter().copied()
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<T: IntoIterator<Item = Capability>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl<const N: usize> From<[Capability; N]> for CapabilitySet {
    fn from(value: [Capability; N]) -> Self {
        value.into_iter().collect()
    }
}

impl IntoIterator for CapabilitySet {
    type Item = Capability;
    type IntoIter = std::collections::btree_set::IntoIter<Capability>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::{Capability, CapabilitySet};

    #[test]
    fn capability_sets_are_deterministic_and_support_subset_checks() {
        let all = CapabilitySet::from([
            Capability::SpawnProcess,
            Capability::ReadWorkspace,
            Capability::WriteWorkspace,
        ]);
        let reads = CapabilitySet::from([Capability::ReadWorkspace]);

        assert!(reads.is_subset(&all));
        assert_eq!(
            all.iter().collect::<Vec<_>>(),
            vec![
                Capability::ReadWorkspace,
                Capability::WriteWorkspace,
                Capability::SpawnProcess,
            ]
        );
    }
}
