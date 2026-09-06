use std::sync::Arc;

use thiserror::Error;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::{
    ToolCall, ToolDispatchContext, ToolDispatchRequest, ToolInvocation, ToolOutcome, ToolRegistry,
};

/// Resolves model-proposed calls to handlers and enforces handler metadata.
///
/// The router never grants capabilities and never executes an action intent.
/// Those responsibilities remain with the kernel action gate.
#[derive(Clone)]
pub struct ToolRouter {
    registry: Arc<ToolRegistry>,
}

impl ToolRouter {
    #[must_use]
    pub const fn new(registry: Arc<ToolRegistry>) -> Self {
        Self { registry }
    }

    #[must_use]
    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    /// Dispatches a call using its model call ID as the idempotency key.
    ///
    /// Durable turn engines should prefer dispatch_with_context and supply a
    /// key scoped by thread and turn.
    pub async fn dispatch(&self, call: ToolCall) -> Result<ToolOutcome, ToolRouteError> {
        let context = ToolDispatchContext::for_call(&call, CancellationToken::new());
        self.dispatch_with_context(call, context).await
    }

    pub async fn dispatch_with_context(
        &self,
        call: ToolCall,
        context: ToolDispatchContext,
    ) -> Result<ToolOutcome, ToolRouteError> {
        if call.id.trim().is_empty() {
            return Err(ToolRouteError::MissingCallId);
        }
        if context.idempotency_key.trim().is_empty() {
            return Err(ToolRouteError::MissingIdempotencyKey);
        }
        let registered =
            self.registry
                .resolve(&call.name)
                .ok_or_else(|| ToolRouteError::UnknownTool {
                    name: call.name.clone(),
                })?;
        if !call.input.arguments.is_object() {
            return Err(ToolRouteError::InvalidInput {
                name: call.name.clone(),
            });
        }
        if context.cancellation.is_cancelled() {
            return Err(ToolRouteError::Cancelled {
                name: call.name.clone(),
            });
        }

        let invocation = ToolInvocation {
            call_id: call.id.clone(),
            input: call.input,
            idempotency_key: context.idempotency_key.clone(),
            cancellation: context.cancellation.clone(),
        };
        let outcome = registered
            .handler
            .invoke(invocation)
            .await
            .map_err(|source| ToolRouteError::HandlerFailed {
                name: call.name.clone(),
                source,
            })?;
        if context.cancellation.is_cancelled() {
            return Err(ToolRouteError::Cancelled {
                name: call.name.clone(),
            });
        }
        validate_outcome(
            &registered.spec,
            &call.id,
            &context.idempotency_key,
            &outcome,
        )?;
        Ok(outcome)
    }

    /// Dispatches a model batch while preserving result order.
    ///
    /// Only consecutive read-only tools explicitly marked parallel-safe are
    /// overlapped. Non-parallel reads and all action handlers form scheduling
    /// barriers and are invoked one at a time.
    pub async fn dispatch_batch(
        &self,
        calls: Vec<ToolCall>,
        max_parallel_reads: usize,
        cancellation: CancellationToken,
    ) -> Vec<Result<ToolOutcome, ToolRouteError>> {
        let requests = calls
            .into_iter()
            .map(|call| {
                let context = ToolDispatchContext::for_call(&call, cancellation.clone());
                ToolDispatchRequest::new(call, context)
            })
            .collect();
        self.dispatch_batch_with_contexts(requests, max_parallel_reads)
            .await
    }

    /// Dispatches a batch with caller-owned durable correlation keys.
    ///
    /// Only consecutive read-only tools explicitly marked parallel-safe are
    /// overlapped. Non-parallel reads and all action handlers form scheduling
    /// barriers and are invoked one at a time. Results retain input order.
    pub async fn dispatch_batch_with_contexts(
        &self,
        requests: Vec<ToolDispatchRequest>,
        max_parallel_reads: usize,
    ) -> Vec<Result<ToolOutcome, ToolRouteError>> {
        let maximum = max_parallel_reads.max(1);
        let mut pending = requests.into_iter().enumerate().peekable();
        let mut completed = Vec::new();

        while let Some((index, request)) = pending.next() {
            if self.call_is_parallel_safe(&request.call) {
                let mut group = vec![(index, request)];
                while group.len() < maximum {
                    let Some((_, next)) = pending.peek() else {
                        break;
                    };
                    if !self.call_is_parallel_safe(&next.call) {
                        break;
                    }
                    if let Some(next) = pending.next() {
                        group.push(next);
                    }
                }
                let mut tasks = JoinSet::new();
                for (group_index, group_request) in group {
                    let router = self.clone();
                    tasks.spawn(async move {
                        let result = router
                            .dispatch_with_context(group_request.call, group_request.context)
                            .await;
                        (group_index, result)
                    });
                }
                while let Some(result) = tasks.join_next().await {
                    match result {
                        Ok(result) => completed.push(result),
                        Err(error) => completed.push((
                            index,
                            Err(ToolRouteError::DispatchTaskFailed {
                                message: error.to_string(),
                            }),
                        )),
                    }
                }
            } else {
                let result = self
                    .dispatch_with_context(request.call, request.context)
                    .await;
                completed.push((index, result));
            }
        }

        completed.sort_by_key(|(index, _)| *index);
        completed.into_iter().map(|(_, result)| result).collect()
    }

    fn call_is_parallel_safe(&self, call: &ToolCall) -> bool {
        self.registry
            .spec(&call.name)
            .is_some_and(crate::ToolSpec::can_run_in_parallel)
    }
}

fn validate_outcome(
    spec: &crate::ToolSpec,
    call_id: &str,
    idempotency_key: &str,
    outcome: &ToolOutcome,
) -> Result<(), ToolRouteError> {
    match outcome {
        ToolOutcome::Evidence(_) if !spec.is_read_only() => {
            Err(ToolRouteError::ActionReturnedEvidence {
                name: spec.name().to_owned(),
            })
        }
        ToolOutcome::ActionIntent(_) if spec.is_read_only() => {
            Err(ToolRouteError::ReadOnlyProposedAction {
                name: spec.name().to_owned(),
            })
        }
        ToolOutcome::Evidence(_) => Ok(()),
        ToolOutcome::ActionIntent(intent) => {
            if intent.tool_call_id != call_id {
                return Err(ToolRouteError::MismatchedCallId {
                    name: spec.name().to_owned(),
                });
            }
            if intent.action != spec.name() {
                return Err(ToolRouteError::MismatchedAction {
                    name: spec.name().to_owned(),
                });
            }
            if intent.idempotency_key != idempotency_key {
                return Err(ToolRouteError::MismatchedIdempotencyKey {
                    name: spec.name().to_owned(),
                });
            }
            if !intent.input.is_object() {
                return Err(ToolRouteError::InvalidActionInput {
                    name: spec.name().to_owned(),
                });
            }
            if !intent
                .required_capabilities
                .is_subset(spec.required_capabilities())
            {
                return Err(ToolRouteError::UndeclaredCapability {
                    name: spec.name().to_owned(),
                });
            }
            if !spec
                .required_capabilities()
                .is_subset(&intent.required_capabilities)
            {
                return Err(ToolRouteError::MissingRequiredCapability {
                    name: spec.name().to_owned(),
                });
            }
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ToolRouteError {
    #[error("工具调用缺少调用 ID")]
    MissingCallId,
    #[error("工具调用缺少幂等键")]
    MissingIdempotencyKey,
    #[error("未知工具：{name}")]
    UnknownTool { name: String },
    #[error("工具 {name} 的参数必须是 JSON 对象")]
    InvalidInput { name: String },
    #[error("工具 {name} 已取消")]
    Cancelled { name: String },
    #[error("工具 {name} 执行失败：{source}")]
    HandlerFailed {
        name: String,
        source: crate::ToolFailure,
    },
    #[error("只读工具 {name} 不能提出副作用操作")]
    ReadOnlyProposedAction { name: String },
    #[error("副作用工具 {name} 必须返回操作意图")]
    ActionReturnedEvidence { name: String },
    #[error("工具 {name} 返回了不匹配的调用 ID")]
    MismatchedCallId { name: String },
    #[error("工具 {name} 返回了不匹配的操作类型")]
    MismatchedAction { name: String },
    #[error("工具 {name} 返回了不匹配的幂等键")]
    MismatchedIdempotencyKey { name: String },
    #[error("工具 {name} 返回的操作参数必须是 JSON 对象")]
    InvalidActionInput { name: String },
    #[error("工具 {name} 提出了其元数据未声明的能力")]
    UndeclaredCapability { name: String },
    #[error("工具 {name} 返回的操作意图遗漏了元数据声明的必需能力")]
    MissingRequiredCapability { name: String },
    #[error("并行工具任务失败：{message}")]
    DispatchTaskFailed { message: String },
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use async_trait::async_trait;
    use serde_json::json;
    use tokio_util::sync::CancellationToken;

    use super::{ToolRouteError, ToolRouter};
    use crate::{
        ActionIntent, Capability, CapabilitySet, Evidence, ToolCall, ToolDispatchContext,
        ToolDispatchRequest, ToolFailure, ToolHandler, ToolInvocation, ToolOutcome, ToolRegistry,
        ToolSpec,
    };

    struct StaticHandler {
        spec: ToolSpec,
        outcome: ToolOutcome,
    }

    struct TrackingHandler {
        spec: ToolSpec,
        active: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ToolHandler for TrackingHandler {
        fn spec(&self) -> &ToolSpec {
            &self.spec
        }

        async fn invoke(&self, invocation: ToolInvocation) -> Result<ToolOutcome, ToolFailure> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(25)).await;
            self.active.fetch_sub(1, Ordering::SeqCst);

            if self.spec.is_read_only() {
                Ok(ToolOutcome::Evidence(Evidence::new(json!({
                    "call_id": invocation.call_id
                }))))
            } else {
                Ok(ToolOutcome::ActionIntent(ActionIntent::new(
                    invocation.call_id,
                    self.spec.name(),
                    invocation.input.arguments,
                    self.spec.required_capabilities().clone(),
                    invocation.idempotency_key,
                )))
            }
        }
    }

    #[async_trait]
    impl ToolHandler for StaticHandler {
        fn spec(&self) -> &ToolSpec {
            &self.spec
        }

        async fn invoke(&self, _invocation: ToolInvocation) -> Result<ToolOutcome, ToolFailure> {
            Ok(self.outcome.clone())
        }
    }

    fn router_with(handler: StaticHandler) -> ToolRouter {
        let mut registry = ToolRegistry::new();
        assert_eq!(registry.register(handler), Ok(()));
        ToolRouter::new(Arc::new(registry))
    }

    #[tokio::test]
    async fn router_dispatches_a_read_only_call() {
        let router = router_with(StaticHandler {
            spec: ToolSpec::read_only(
                "list_files",
                "列出文件",
                json!({"type": "object"}),
                CapabilitySet::from([Capability::ReadWorkspace]),
                true,
            ),
            outcome: ToolOutcome::Evidence(Evidence::new(json!({"files": ["README.md"]}))),
        });

        let outcome = router
            .dispatch(ToolCall::new("call-1", "list_files", json!({})))
            .await;
        assert!(matches!(outcome, Ok(ToolOutcome::Evidence(_))));
    }

    #[tokio::test]
    async fn router_rejects_an_action_from_a_read_only_handler() {
        let router = router_with(StaticHandler {
            spec: ToolSpec::read_only(
                "unsafe_read",
                "错误声明的工具",
                json!({"type": "object"}),
                CapabilitySet::from([Capability::ReadWorkspace]),
                false,
            ),
            outcome: ToolOutcome::ActionIntent(ActionIntent::new(
                "call-2",
                "unsafe_read",
                json!({"path": "README.md"}),
                CapabilitySet::from([Capability::WriteWorkspace]),
                "call-2",
            )),
        });

        assert!(matches!(
            router
                .dispatch(ToolCall::new("call-2", "unsafe_read", json!({})))
                .await,
            Err(ToolRouteError::ReadOnlyProposedAction { .. })
        ));
    }

    #[tokio::test]
    async fn router_rejects_capabilities_missing_from_tool_metadata() {
        let router = router_with(StaticHandler {
            spec: ToolSpec::action(
                "write_file",
                "写入文件",
                json!({"type": "object"}),
                CapabilitySet::from([Capability::WriteWorkspace]),
            ),
            outcome: ToolOutcome::ActionIntent(ActionIntent::new(
                "call-3",
                "write_file",
                json!({"path": "README.md"}),
                CapabilitySet::from([Capability::WriteWorkspace, Capability::AccessNetwork]),
                "call-3",
            )),
        });

        assert!(matches!(
            router
                .dispatch(ToolCall::new("call-3", "write_file", json!({})))
                .await,
            Err(ToolRouteError::UndeclaredCapability { .. })
        ));
    }

    #[tokio::test]
    async fn router_rejects_action_intents_that_omit_required_capabilities() {
        let router = router_with(StaticHandler {
            spec: ToolSpec::action(
                "run_command",
                "运行命令",
                json!({"type": "object"}),
                CapabilitySet::from([Capability::SpawnProcess, Capability::AccessNetwork]),
            ),
            outcome: ToolOutcome::ActionIntent(ActionIntent::new(
                "call-missing-capability",
                "run_command",
                json!({}),
                CapabilitySet::from([Capability::SpawnProcess]),
                "call-missing-capability",
            )),
        });

        assert!(matches!(
            router
                .dispatch(ToolCall::new(
                    "call-missing-capability",
                    "run_command",
                    json!({})
                ))
                .await,
            Err(ToolRouteError::MissingRequiredCapability { .. })
        ));
    }

    #[tokio::test]
    async fn cancellation_and_non_object_inputs_fail_before_handler_execution() {
        let router = router_with(StaticHandler {
            spec: ToolSpec::read_only(
                "read_file",
                "读取文件",
                json!({"type": "object"}),
                CapabilitySet::from([Capability::ReadWorkspace]),
                true,
            ),
            outcome: ToolOutcome::Evidence(Evidence::new(json!(null))),
        });
        assert!(matches!(
            router
                .dispatch(ToolCall::new("call-4", "read_file", json!("bad")))
                .await,
            Err(ToolRouteError::InvalidInput { .. })
        ));

        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let call = ToolCall::new("call-5", "read_file", json!({}));
        let context = ToolDispatchContext::new("turn/call-5", cancellation);
        assert!(matches!(
            router.dispatch_with_context(call, context).await,
            Err(ToolRouteError::Cancelled { .. })
        ));
    }

    #[tokio::test]
    async fn action_intent_must_retain_durable_correlation() {
        let router = router_with(StaticHandler {
            spec: ToolSpec::action(
                "write_file",
                "写入文件",
                json!({"type": "object"}),
                CapabilitySet::from([Capability::WriteWorkspace]),
            ),
            outcome: ToolOutcome::ActionIntent(ActionIntent::new(
                "wrong-call",
                "write_file",
                json!({}),
                CapabilitySet::from([Capability::WriteWorkspace]),
                "turn/call-6",
            )),
        });
        let call = ToolCall::new("call-6", "write_file", json!({}));
        let context = ToolDispatchContext::new("turn/call-6", CancellationToken::new());

        assert!(matches!(
            router.dispatch_with_context(call, context).await,
            Err(ToolRouteError::MismatchedCallId { .. })
        ));
    }

    #[tokio::test]
    async fn batch_parallelizes_only_safe_reads_and_serializes_actions() {
        let read_active = Arc::new(AtomicUsize::new(0));
        let read_peak = Arc::new(AtomicUsize::new(0));
        let action_active = Arc::new(AtomicUsize::new(0));
        let action_peak = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new();
        assert_eq!(
            registry.register(TrackingHandler {
                spec: ToolSpec::read_only(
                    "read",
                    "读取",
                    json!({"type": "object"}),
                    CapabilitySet::from([Capability::ReadWorkspace]),
                    true,
                ),
                active: read_active,
                peak: read_peak.clone(),
            }),
            Ok(())
        );
        assert_eq!(
            registry.register(TrackingHandler {
                spec: ToolSpec::action(
                    "write",
                    "写入",
                    json!({"type": "object"}),
                    CapabilitySet::from([Capability::WriteWorkspace]),
                ),
                active: action_active,
                peak: action_peak.clone(),
            }),
            Ok(())
        );
        let router = ToolRouter::new(Arc::new(registry));
        let calls = vec![
            ToolCall::new("read-1", "read", json!({})),
            ToolCall::new("read-2", "read", json!({})),
            ToolCall::new("write-1", "write", json!({})),
            ToolCall::new("write-2", "write", json!({})),
        ];

        let results = router
            .dispatch_batch(calls, 2, CancellationToken::new())
            .await;

        assert_eq!(read_peak.load(Ordering::SeqCst), 2);
        assert_eq!(action_peak.load(Ordering::SeqCst), 1);
        assert!(matches!(results[0], Ok(ToolOutcome::Evidence(_))));
        assert!(matches!(results[1], Ok(ToolOutcome::Evidence(_))));
        assert!(matches!(results[2], Ok(ToolOutcome::ActionIntent(_))));
        assert!(matches!(results[3], Ok(ToolOutcome::ActionIntent(_))));
    }

    #[tokio::test]
    async fn batch_serializes_reads_not_marked_parallel_safe() {
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new();
        assert_eq!(
            registry.register(TrackingHandler {
                spec: ToolSpec::read_only(
                    "ordered_read",
                    "顺序读取",
                    json!({"type": "object"}),
                    CapabilitySet::from([Capability::ReadWorkspace]),
                    false,
                ),
                active,
                peak: peak.clone(),
            }),
            Ok(())
        );
        let router = ToolRouter::new(Arc::new(registry));

        let results = router
            .dispatch_batch(
                vec![
                    ToolCall::new("read-1", "ordered_read", json!({})),
                    ToolCall::new("read-2", "ordered_read", json!({})),
                ],
                2,
                CancellationToken::new(),
            )
            .await;

        assert_eq!(peak.load(Ordering::SeqCst), 1);
        assert!(results.iter().all(Result::is_ok));
    }

    #[tokio::test]
    async fn batch_retains_scoped_idempotency_keys_and_result_order() {
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new();
        assert_eq!(
            registry.register(TrackingHandler {
                spec: ToolSpec::action(
                    "write",
                    "写入",
                    json!({"type": "object"}),
                    CapabilitySet::from([Capability::WriteWorkspace]),
                ),
                active,
                peak,
            }),
            Ok(())
        );
        let router = ToolRouter::new(Arc::new(registry));
        let requests = [
            ("call-1", "thread/turn/step/call-1"),
            ("call-2", "thread/turn/step/call-2"),
        ]
        .into_iter()
        .map(|(call_id, key)| {
            ToolDispatchRequest::new(
                ToolCall::new(call_id, "write", json!({})),
                ToolDispatchContext::new(key, CancellationToken::new()),
            )
        })
        .collect();

        let results = router.dispatch_batch_with_contexts(requests, 4).await;
        let intents = results
            .into_iter()
            .map(|result| match result {
                Ok(ToolOutcome::ActionIntent(intent)) => intent,
                other => panic!("expected action intent, got {other:?}"),
            })
            .collect::<Vec<_>>();

        assert_eq!(intents[0].tool_call_id, "call-1");
        assert_eq!(intents[0].idempotency_key, "thread/turn/step/call-1");
        assert_eq!(intents[1].tool_call_id, "call-2");
        assert_eq!(intents[1].idempotency_key, "thread/turn/step/call-2");
    }
}
