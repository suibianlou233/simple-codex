//! Simple-owned implementation of the pinned Codex origin policy and grant scopes.
//! Grants are process-local; persisted rules never persist an approval implicitly.
use super::*;
use std::collections::BTreeMap;
use tokio::sync::oneshot;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Policy {
    pub allow_history_access: bool,
    pub deny_all: bool,
    pub denied_origins: Vec<String>,
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Once,
    Turn,
    Thread,
    Deny,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingView {
    id: String,
    task_id: String,
    origin: String,
    action: String,
    url: String,
    selector: Option<String>,
    text: Option<String>,
}

struct Pending {
    view: PendingView,
    project: String,
    token: CancellationToken,
    sender: oneshot::Sender<Scope>,
}
struct Grant {
    project: String,
    instance: String,
    task: String,
    turn: Option<String>,
    origin: String,
}

#[derive(Default)]
pub struct AccessState {
    policies: BTreeMap<String, Policy>,
    loaded: bool,
    pending: BTreeMap<String, Pending>,
    grants: Vec<Grant>,
    revision: u64,
}

fn origin(url: &str) -> Result<String, String> {
    Ok(web_url(url)?.origin().ascii_serialization())
}

fn policy_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|_| err("浏览器权限目录不可用"))?
        .join("browser-policy.json"))
}

impl AccessState {
    fn load(&mut self, app: &AppHandle) -> Result<(), String> {
        if self.loaded {
            return Ok(());
        }
        let path = policy_path(app)?;
        match fs::File::open(path) {
            Ok(file) => {
                use std::io::Read;
                let mut data = Vec::new();
                file.take(1_048_577)
                    .read_to_end(&mut data)
                    .map_err(|_| err("无法读取浏览器权限"))?;
                if data.len() > 1_048_576 {
                    return Err(err("浏览器权限文件过大"));
                }
                self.policies = serde_json::from_slice(&data)
                    .map_err(|_| err("浏览器权限文件无效，请修复后重试"))?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err(err("无法读取浏览器权限")),
        }
        self.loaded = true;
        Ok(())
    }

    fn check(&self, project: &str, site: &str, action: &str) -> Result<(), String> {
        let policy = self.policies.get(project).cloned().unwrap_or_default();
        if policy.deny_all || policy.denied_origins.iter().any(|value| value == site) {
            return Err(err("该网站的 AI 浏览器访问已被禁用"));
        }
        if matches!(action, "back" | "forward") && !policy.allow_history_access {
            return Err(err("未允许 AI 访问浏览历史，请直接打开目标地址"));
        }
        Ok(())
    }

    fn granted(
        &self,
        project: &str,
        instance: &str,
        binding: &CodexTurnBinding,
        site: &str,
    ) -> bool {
        self.grants.iter().any(|grant| {
            grant.project == project
                && grant.instance == instance
                && grant.task == binding.task_id
                && grant.origin == site
                && grant
                    .turn
                    .as_ref()
                    .is_none_or(|turn| turn == &binding.turn_id)
        })
    }

    pub(super) fn finish(&mut self, turn: &str) {
        self.grants
            .retain(|grant| grant.turn.as_deref() != Some(turn));
        self.pending
            .retain(|_, pending| !pending.token.is_cancelled());
    }

    fn revoke(&mut self, project: &str) {
        self.revision += 1;
        self.grants.retain(|grant| grant.project != project);
        self.pending.retain(|_, pending| pending.project != project);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessView {
    policy: Policy,
    grants: usize,
}

#[tauri::command]
pub fn browser_access_pending(
    window: Webview,
    browser: State<'_, BrowserState>,
) -> Result<Vec<PendingView>, String> {
    if window.label() != "main" {
        return Err(err("仅主窗口可以查看浏览器确认"));
    }
    let state = browser.access.lock().map_err(|_| err("浏览器权限不可用"))?;
    Ok(state
        .pending
        .values()
        .filter(|p| !p.token.is_cancelled())
        .map(|p| p.view.clone())
        .collect())
}

#[tauri::command]
pub fn browser_access_resolve(
    window: Webview,
    id: String,
    scope: Scope,
    browser: State<'_, BrowserState>,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err(err("仅主窗口可以确认浏览器操作"));
    }
    let mut state = browser.access.lock().map_err(|_| err("浏览器权限不可用"))?;
    let pending = state
        .pending
        .remove(&id)
        .ok_or_else(|| err("该确认已结束"))?;
    if pending.token.is_cancelled() {
        return Err(err("任务已停止"));
    }
    pending.sender.send(scope).map_err(|_| err("该确认已失效"))
}

fn persist_policy(path: &Path, next: &BTreeMap<String, Policy>) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| err("权限路径无效"))?;
    fs::create_dir_all(parent).map_err(|_| err("无法保存浏览器权限"))?;
    let temp = parent.join(format!("browser-policy-{}.tmp", Uuid::new_v4()));
    use std::io::Write;
    let bytes = serde_json::to_vec(next).map_err(|_| err("无法保存浏览器权限"))?;
    if bytes.len() > 1_048_576 {
        return Err(err("浏览器权限配置过大"));
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|_| err("无法保存浏览器权限"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| err("无法保存浏览器权限"))?;
    drop(file);
    fs::rename(&temp, &path).map_err(|_| err("无法提交浏览器权限"))?;
    Ok(())
}

#[tauri::command]
pub fn browser_access_policy(
    window: Webview,
    app: AppHandle,
    project_id: String,
    policy: Option<Policy>,
    revoke: Option<bool>,
    browser: State<'_, BrowserState>,
    desktop: State<'_, DesktopState>,
) -> Result<AccessView, String> {
    if window.label() != "main" {
        return Err(err("仅主窗口可以修改浏览器权限"));
    }
    let project = desktop
        .lock()
        .map_err(command_error)?
        .project_root(&project_id)
        .map_err(command_error)?;
    let key = project_key(&project)?;
    let mut state = browser.access.lock().map_err(|_| err("浏览器权限不可用"))?;
    state.load(&app)?;
    if let Some(mut policy) = policy {
        if policy.denied_origins.len() > 100 {
            return Err(err("最多配置 100 个网站"));
        }
        policy.denied_origins = policy
            .denied_origins
            .iter()
            .map(|url| origin(url))
            .collect::<Result<_, _>>()?;
        policy.denied_origins.sort();
        policy.denied_origins.dedup();
        let mut next = state.policies.clone();
        next.insert(key.clone(), policy);
        let path = policy_path(&app)?;
        persist_policy(&path, &next)?;
        state.policies = next;
        state.revoke(&key);
    }
    if revoke.unwrap_or(false) {
        state.revoke(&key);
    }
    Ok(AccessView {
        policy: state.policies.get(&key).cloned().unwrap_or_default(),
        grants: state.grants.iter().filter(|g| g.project == key).count(),
    })
}

pub(super) async fn authorize(
    app: &AppHandle,
    project: &Path,
    instance: &str,
    binding: &CodexTurnBinding,
    request: &BrowserRequest,
    target: &str,
    token: &CancellationToken,
) -> Result<(String, u64), String> {
    let key = project_key(project)?;
    let site = origin(target)?;
    let browser = app.state::<BrowserState>();
    let id = Uuid::new_v4().to_string();
    let (sender, receiver) = oneshot::channel();
    let revision = {
        let mut state = browser.access.lock().map_err(|_| err("浏览器权限不可用"))?;
        state.load(app)?;
        state.check(&key, &site, &request.action)?;
        if state.granted(&key, instance, binding, &site) {
            return Ok(("cached".into(), state.revision));
        }
        let view = PendingView {
            id: id.clone(),
            task_id: binding.task_id.clone(),
            origin: site.clone(),
            action: request.action.clone(),
            url: target.to_owned(),
            selector: request.selector.clone(),
            text: request.text.clone(),
        };
        state.pending.insert(
            id.clone(),
            Pending {
                view,
                project: key.clone(),
                token: token.clone(),
                sender,
            },
        );
        state.revision
    };
    let response = tokio::select! {
        biased;
        _ = token.cancelled() => Err(err("任务已停止，浏览器请求已取消")),
        response = receiver => response.map_err(|_| err("浏览器授权已撤销")),
    };
    let mut state = browser.access.lock().map_err(|_| err("浏览器权限不可用"))?;
    state.pending.remove(&id);
    let scope = response?;
    if token.is_cancelled() || revision != state.revision {
        return Err(err("浏览器授权已失效"));
    }
    state.check(&key, &site, &request.action)?;
    if scope == Scope::Deny {
        return Err(err("用户拒绝了本次浏览器操作"));
    }
    if matches!(scope, Scope::Turn | Scope::Thread) {
        state.grants.push(Grant {
            project: key,
            instance: instance.into(),
            task: binding.task_id.clone(),
            turn: (scope == Scope::Turn).then(|| binding.turn_id.clone()),
            origin: site,
        });
    }
    Ok((
        match scope {
            Scope::Once => "once",
            Scope::Turn => "turn",
            Scope::Thread => "thread",
            Scope::Deny => "deny",
        }
        .into(),
        revision,
    ))
}

pub(super) fn recheck(
    app: &AppHandle,
    project: &Path,
    target: &str,
    action: &str,
    revision: u64,
) -> Result<(), String> {
    let browser = app.state::<BrowserState>();
    let state = browser.access.lock().map_err(|_| err("浏览器权限不可用"))?;
    if state.revision != revision {
        return Err(err("浏览器权限已改变，请重新请求"));
    }
    state.check(&project_key(project)?, &origin(target)?, action)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_and_revocation_close_pending_approval_channels() {
        for cancel in [true, false] {
            let mut state = AccessState::default();
            let token = CancellationToken::new();
            let (sender, mut receiver) = oneshot::channel();
            state.pending.insert(
                "id".into(),
                Pending {
                    project: "p".into(),
                    token: token.clone(),
                    sender,
                    view: PendingView {
                        id: "id".into(),
                        task_id: "s".into(),
                        origin: "https://example.com".into(),
                        action: "read".into(),
                        url: "https://example.com".into(),
                        selector: None,
                        text: None,
                    },
                },
            );
            if cancel {
                token.cancel();
                state.finish("t");
            } else {
                state.revoke("p");
            }
            assert!(state.pending.is_empty());
            assert!(matches!(
                receiver.try_recv(),
                Err(oneshot::error::TryRecvError::Closed)
            ));
        }
    }
    #[test]
    fn persistent_rules_replace_atomically_without_saving_grants() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("policy.json");
        let mut policies = BTreeMap::new();
        policies.insert("project".into(), Policy::default());
        persist_policy(&path, &policies).unwrap();
        policies.get_mut("project").unwrap().deny_all = true;
        persist_policy(&path, &policies).unwrap();
        let bytes = fs::read(path).unwrap();
        let restored: BTreeMap<String, Policy> = serde_json::from_slice(&bytes).unwrap();
        assert!(restored["project"].deny_all);
        assert!(!String::from_utf8(bytes).unwrap().contains("grants"));
        assert!(serde_json::from_value::<Policy>(json!({"allowHistoryAccess":true,"denyAll":false,"deniedOrigins":[],"fullCdpAccess":true})).is_err());
    }
    #[test]
    fn grants_do_not_cross_origin_task_instance_or_turn() {
        let mut state = AccessState::default();
        let mut binding = CodexTurnBinding {
            task_id: "s".into(),
            turn_id: "t".into(),
            codex_thread_id: "n".into(),
            codex_turn_id: "nt".into(),
        };
        state.grants.push(Grant {
            project: "p".into(),
            instance: "i".into(),
            task: "s".into(),
            turn: Some("t".into()),
            origin: "https://example.com".into(),
        });
        assert!(state.granted("p", "i", &binding, "https://example.com"));
        for (project, instance, site) in [
            ("other", "i", "https://example.com"),
            ("p", "other", "https://example.com"),
            ("p", "i", "https://other.com"),
        ] {
            assert!(!state.granted(project, instance, &binding, site));
        }
        binding.turn_id = "other-turn".into();
        assert!(!state.granted("p", "i", &binding, "https://example.com"));
        state.grants[0].turn = None;
        assert!(state.granted("p", "i", &binding, "https://example.com"));
        binding.task_id = "other-task".into();
        assert!(!state.granted("p", "i", &binding, "https://example.com"));
    }
    #[test]
    fn origin_and_history_policy_is_exact_and_fail_closed() {
        assert_eq!(
            origin("https://EXAMPLE.com:443/path?q=x").unwrap(),
            "https://example.com"
        );
        let mut state = AccessState::default();
        assert!(state.check("p", "https://example.com", "back").is_err());
        state.policies.insert(
            "p".into(),
            Policy {
                denied_origins: vec!["https://example.com".into()],
                ..Policy::default()
            },
        );
        assert!(state.check("p", "https://example.com", "read").is_err());
        assert!(state.check("p", "https://example.com.evil", "read").is_ok());
        assert!(state.check("p", "https://example.com:8443", "read").is_ok());
    }
    #[test]
    fn turn_cleanup_and_project_revocation_preserve_other_scopes() {
        let mut state = AccessState::default();
        for turn in [Some("t".into()), None] {
            state.grants.push(Grant {
                project: "p".into(),
                instance: "i".into(),
                task: "s".into(),
                turn,
                origin: "https://example.com".into(),
            });
        }
        state.finish("t");
        assert_eq!(state.grants.len(), 1);
        state.revoke("other");
        assert_eq!(state.grants.len(), 1);
        state.revoke("p");
        assert!(state.grants.is_empty());
    }
}
