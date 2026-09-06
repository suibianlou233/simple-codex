use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;

use crate::{ToolFailure, ToolInvocation, ToolOutcome, ToolSpec, ToolSpecError};

#[async_trait]
pub trait ToolHandler: Send + Sync {
    fn spec(&self) -> &ToolSpec;

    /// Reads evidence or prepares an action intent.
    ///
    /// Implementations declared as action tools must not perform the action.
    /// The router validates the returned intent before it reaches the action
    /// gate, which owns authorization, persistence, and execution.
    async fn invoke(&self, invocation: ToolInvocation) -> Result<ToolOutcome, ToolFailure>;
}

#[derive(Clone)]
pub(crate) struct RegisteredTool {
    pub spec: ToolSpec,
    pub handler: Arc<dyn ToolHandler>,
}

/// An immutable-at-use catalog of uniquely named tool handlers.
#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, RegisteredTool>,
}

impl ToolRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<H>(&mut self, handler: H) -> Result<(), ToolRegistryError>
    where
        H: ToolHandler + 'static,
    {
        self.register_shared(Arc::new(handler))
    }

    pub fn register_shared(
        &mut self,
        handler: Arc<dyn ToolHandler>,
    ) -> Result<(), ToolRegistryError> {
        let spec = handler.spec().clone();
        spec.validate().map_err(ToolRegistryError::InvalidSpec)?;
        let name = spec.name().to_owned();
        if self.tools.contains_key(&name) {
            return Err(ToolRegistryError::DuplicateName { name });
        }
        self.tools.insert(name, RegisteredTool { spec, handler });
        Ok(())
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    #[must_use]
    pub fn spec(&self, name: &str) -> Option<&ToolSpec> {
        self.tools.get(name).map(|tool| &tool.spec)
    }

    pub fn specs(&self) -> impl ExactSizeIterator<Item = &ToolSpec> {
        self.tools.values().map(|tool| &tool.spec)
    }

    pub fn parallel_read_only_specs(&self) -> impl Iterator<Item = &ToolSpec> {
        self.specs().filter(|spec| spec.can_run_in_parallel())
    }

    pub fn action_specs(&self) -> impl Iterator<Item = &ToolSpec> {
        self.specs().filter(|spec| !spec.is_read_only())
    }

    pub(crate) fn resolve(&self, name: &str) -> Option<RegisteredTool> {
        self.tools.get(name).cloned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ToolRegistryError {
    #[error("工具定义无效：{0}")]
    InvalidSpec(ToolSpecError),
    #[error("工具名称重复：{name}")]
    DuplicateName { name: String },
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use serde_json::json;

    use super::{ToolHandler, ToolRegistry, ToolRegistryError};
    use crate::{
        Capability, CapabilitySet, Evidence, ToolFailure, ToolInvocation, ToolOutcome, ToolSpec,
    };

    struct ReadHandler {
        spec: ToolSpec,
    }

    impl ReadHandler {
        fn new() -> Self {
            Self {
                spec: ToolSpec::read_only(
                    "read_file",
                    "读取文件",
                    json!({"type": "object"}),
                    CapabilitySet::from([Capability::ReadWorkspace]),
                    true,
                ),
            }
        }
    }

    #[async_trait]
    impl ToolHandler for ReadHandler {
        fn spec(&self) -> &ToolSpec {
            &self.spec
        }

        async fn invoke(&self, invocation: ToolInvocation) -> Result<ToolOutcome, ToolFailure> {
            Ok(ToolOutcome::Evidence(Evidence::new(
                invocation.input.arguments,
            )))
        }
    }

    #[test]
    fn duplicate_names_are_rejected_without_replacing_the_first_handler() {
        let mut registry = ToolRegistry::new();
        registry
            .register(ReadHandler::new())
            .expect("first registration should pass");
        let result = registry.register(ReadHandler::new());

        assert_eq!(
            result,
            Err(ToolRegistryError::DuplicateName {
                name: "read_file".to_owned()
            })
        );
        assert_eq!(registry.len(), 1);
        assert!(registry.spec("read_file").is_some());
    }

    #[test]
    fn parallel_catalog_only_contains_parallel_safe_read_tools() {
        let mut registry = ToolRegistry::new();
        registry
            .register(ReadHandler::new())
            .expect("registration should pass");

        assert_eq!(
            registry
                .parallel_read_only_specs()
                .map(ToolSpec::name)
                .collect::<Vec<_>>(),
            vec!["read_file"]
        );
    }
}
