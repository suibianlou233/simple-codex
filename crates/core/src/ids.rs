use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! domain_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

domain_id!(ProjectId);
domain_id!(TaskId);
domain_id!(TurnId);
domain_id!(StepId);
domain_id!(ItemId);

/// Kernel terminology. The desktop bridge still exposes the historical
/// project/task names, but both pairs identify the same local concepts.
pub type WorkspaceId = ProjectId;
pub type ThreadId = TaskId;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::TaskId;

    #[test]
    fn id_is_a_json_string_and_round_trips() {
        let id = TaskId::new();
        let json = serde_json::to_string(&id).expect("id should serialize");
        let decoded: TaskId = serde_json::from_str(&json).expect("id should deserialize");

        assert_eq!(decoded, id);
        assert!(json.starts_with('"'));
    }
}
