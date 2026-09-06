use local_agent_tools::{ActionIntent, Capability, CapabilitySet};
use serde::{Deserialize, Serialize};

/// User authorization and the enforceable sandbox boundary are deliberately
/// separate. A user grant cannot manufacture an isolation capability that the
/// current host does not provide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityBoundary {
    pub user_grant: CapabilitySet,
    pub enforced_boundary: CapabilitySet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalPolicy {
    RequireExplicit,
    AlreadyApproved,
    NotRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionGateDecision {
    AwaitApproval,
    Execute,
    Denied {
        capability: Capability,
        reason: CapabilityDenial,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityDenial {
    NotGrantedByUser,
    NotEnforcedBySandbox,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ActionGate;

impl ActionGate {
    #[must_use]
    pub fn evaluate(
        intent: &ActionIntent,
        boundary: &CapabilityBoundary,
        approval: ApprovalPolicy,
    ) -> ActionGateDecision {
        for capability in intent.required_capabilities.iter() {
            if !boundary.user_grant.contains(capability) {
                return ActionGateDecision::Denied {
                    capability,
                    reason: CapabilityDenial::NotGrantedByUser,
                };
            }
            if !boundary.enforced_boundary.contains(capability) {
                return ActionGateDecision::Denied {
                    capability,
                    reason: CapabilityDenial::NotEnforcedBySandbox,
                };
            }
        }
        match approval {
            ApprovalPolicy::RequireExplicit => ActionGateDecision::AwaitApproval,
            ApprovalPolicy::AlreadyApproved | ApprovalPolicy::NotRequired => {
                ActionGateDecision::Execute
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use local_agent_tools::{ActionIntent, Capability, CapabilitySet};
    use serde_json::json;

    use super::{
        ActionGate, ActionGateDecision, ApprovalPolicy, CapabilityBoundary, CapabilityDenial,
    };

    fn intent(capabilities: CapabilitySet) -> ActionIntent {
        ActionIntent::new("call", "write_file", json!({}), capabilities, "turn/call")
    }

    #[test]
    fn user_authorization_cannot_bypass_real_sandbox_boundary() {
        let intent = intent(CapabilitySet::from([Capability::WriteOutsideWorkspace]));
        let decision = ActionGate::evaluate(
            &intent,
            &CapabilityBoundary {
                user_grant: CapabilitySet::from([Capability::WriteOutsideWorkspace]),
                enforced_boundary: CapabilitySet::from([Capability::WriteWorkspace]),
            },
            ApprovalPolicy::AlreadyApproved,
        );
        assert_eq!(
            decision,
            ActionGateDecision::Denied {
                capability: Capability::WriteOutsideWorkspace,
                reason: CapabilityDenial::NotEnforcedBySandbox,
            }
        );
    }

    #[test]
    fn approval_is_a_pause_not_an_execution_authority() {
        let capabilities = CapabilitySet::from([Capability::WriteWorkspace]);
        let boundary = CapabilityBoundary {
            user_grant: capabilities.clone(),
            enforced_boundary: capabilities.clone(),
        };
        assert_eq!(
            ActionGate::evaluate(
                &intent(capabilities),
                &boundary,
                ApprovalPolicy::RequireExplicit
            ),
            ActionGateDecision::AwaitApproval
        );
    }
}
