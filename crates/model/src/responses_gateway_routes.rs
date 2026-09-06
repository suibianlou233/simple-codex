//! Immutable, process-local model revisions. An old task never follows a newer
//! task's credential, endpoint or protocol merely because settings were saved.
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use reqwest::{Client, Url};

use super::{ResponsesGatewayConfig, ResponsesGatewayError, ResponsesGatewayUpstream};
use crate::ApiKey;

const MAX_ROUTES: usize = 256;

pub(super) struct Route {
    pub client: Client,
    pub upstream_url: Url,
    pub upstream_model: String,
    pub upstream_api_key: Option<ApiKey>,
    pub upstream_protocol: ResponsesGatewayUpstream,
    config: ResponsesGatewayConfig,
}

pub(super) struct Routes {
    entries: RwLock<HashMap<String, Arc<Route>>>,
    max_request_bytes: usize,
}

fn invalid(message: &str) -> ResponsesGatewayError {
    ResponsesGatewayError::InvalidConfiguration(message.to_owned())
}

impl Routes {
    pub fn new(config: ResponsesGatewayConfig) -> Result<Self, ResponsesGatewayError> {
        let routes = Self {
            entries: RwLock::new(HashMap::new()),
            max_request_bytes: config.max_request_bytes,
        };
        routes.register(config)?;
        Ok(routes)
    }

    pub fn register(&self, config: ResponsesGatewayConfig) -> Result<(), ResponsesGatewayError> {
        let alias = config.codex_model_alias.trim();
        if alias.is_empty() || alias.len() > 256 || alias != config.codex_model_alias {
            return Err(invalid("Codex 模型别名无效"));
        }
        if config.upstream_model.trim().is_empty() {
            return Err(invalid("模型名称不能为空"));
        }
        if config.max_request_bytes == 0 || config.max_request_bytes != self.max_request_bytes {
            return Err(invalid("同一网关的请求大小限制必须一致且大于 0"));
        }
        let mut entries = self
            .entries
            .write()
            .map_err(|_| invalid("模型路由不可用"))?;
        if let Some(existing) = entries.get(alias) {
            let old = &existing.config;
            if old.upstream_base_url == config.upstream_base_url
                && old.upstream_model == config.upstream_model
                && old.upstream_protocol == config.upstream_protocol
                && old.upstream_timeout == config.upstream_timeout
                && old.upstream_api_key.as_ref().map(ApiKey::expose)
                    == config.upstream_api_key.as_ref().map(ApiKey::expose)
            {
                return Ok(());
            }
            return Err(invalid("已使用的模型版本不能覆盖，请重新保存模型配置"));
        }
        if entries.len() >= MAX_ROUTES {
            return Err(invalid(
                "本次运行的模型配置版本过多，请在任务结束后重启 Simple",
            ));
        }
        let upstream_url = match config.upstream_protocol {
            ResponsesGatewayUpstream::Responses | ResponsesGatewayUpstream::DeepSeekResponses => {
                super::responses_url(&config.upstream_base_url)?
            }
            ResponsesGatewayUpstream::ChatCompletions { .. } => {
                super::chat_completions_url(&config.upstream_base_url)?
            }
        };
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(config.upstream_timeout);
        let client = if crate::proxy_policy::is_loopback_endpoint(&upstream_url) {
            client.no_proxy()
        } else {
            client
        };
        let route = Route {
            client: client.build().map_err(ResponsesGatewayError::Client)?,
            upstream_url,
            upstream_model: config.upstream_model.trim().to_owned(),
            upstream_api_key: config.upstream_api_key.clone(),
            upstream_protocol: config.upstream_protocol,
            config: config.clone(),
        };
        entries.insert(alias.to_owned(), Arc::new(route));
        Ok(())
    }

    pub fn get(&self, alias: &str) -> Result<Option<Arc<Route>>, ResponsesGatewayError> {
        Ok(self
            .entries
            .read()
            .map_err(|_| invalid("模型路由不可用"))?
            .get(alias)
            .cloned())
    }
}
