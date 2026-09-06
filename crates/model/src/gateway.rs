use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::{ModelAdapter, ModelCapabilities, ModelError, ModelEventStream, ModelRequest};

/// Provider-neutral model boundary used by the turn engine.
///
/// Provider quirks remain in adapters; orchestration sees one streaming,
/// cancellation, capability, and error surface.
#[derive(Clone)]
pub struct ModelGateway {
    adapter: Arc<dyn ModelAdapter>,
}

impl ModelGateway {
    #[must_use]
    pub fn new<A>(adapter: A) -> Self
    where
        A: ModelAdapter + 'static,
    {
        Self {
            adapter: Arc::new(adapter),
        }
    }

    #[must_use]
    pub fn from_shared(adapter: Arc<dyn ModelAdapter>) -> Self {
        Self { adapter }
    }

    #[must_use]
    pub fn capabilities(&self) -> ModelCapabilities {
        self.adapter.capabilities()
    }

    pub async fn stream(
        &self,
        request: ModelRequest,
        cancellation: CancellationToken,
    ) -> Result<ModelEventStream, ModelError> {
        if cancellation.is_cancelled() {
            return Err(ModelError::Cancelled);
        }
        self.adapter.stream(request, cancellation).await
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use futures_util::stream;
    use tokio_util::sync::CancellationToken;

    use super::ModelGateway;
    use crate::{
        ModelAdapter, ModelCapabilities, ModelError, ModelEventStream, ModelRequest, ReasoningMode,
    };

    struct NeverCalled;

    #[async_trait]
    impl ModelAdapter for NeverCalled {
        fn capabilities(&self) -> ModelCapabilities {
            ModelCapabilities {
                streaming: true,
                reasoning_control: true,
                tool_calls: true,
            }
        }

        async fn stream(
            &self,
            _request: ModelRequest,
            _cancellation: CancellationToken,
        ) -> Result<ModelEventStream, ModelError> {
            Ok(Box::pin(stream::empty()))
        }
    }

    #[tokio::test]
    async fn cancelled_request_is_rejected_at_the_gateway_boundary() {
        let gateway = ModelGateway::new(NeverCalled);
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let request = ModelRequest {
            messages: Vec::new(),
            reasoning: ReasoningMode::Off,
            max_output_tokens: None,
            tools: Vec::new(),
            tool_history: Vec::new(),
        };
        assert!(matches!(
            gateway.stream(request, cancellation).await,
            Err(ModelError::Cancelled)
        ));
        assert!(gateway.capabilities().tool_calls);
    }
}
