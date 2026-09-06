//! Attribute native child approvals to a live local root, without granting
//! permissions or registering a child projector that could finish that root.
use super::*;

#[derive(Clone)]
pub(super) struct Descendant {
    root: CodexTurnBinding,
    binding: CodexTurnBinding,
    instance: String,
}

impl DesktopRuntime {
    fn owns_native_root(&self, root: &CodexTurnBinding, instance: &str) -> bool {
        self.codex_turn_links.get(&root.codex_turn_id) == Some(root)
            && self.project_leases.contains_key(&root.turn_id)
            && self
                .codex_turn_owners
                .get(&root.turn_id)
                .is_some_and(|owner| owner.instance_id == instance)
            && self.core.snapshot().turns.iter().any(|turn| {
                turn.id.to_string() == root.turn_id && turn.status == TurnStatus::Running
            })
    }

    pub(super) fn codex_action_binding(
        &self,
        thread: &str,
        turn: &str,
    ) -> Option<CodexTurnBinding> {
        self.codex_turn_projectors
            .get(turn)
            .map(CodexTurnProjector::binding)
            .filter(|binding| binding.codex_thread_id == thread)
            .cloned()
            .or_else(|| {
                self.codex_descendants
                    .get(&(thread.to_owned(), turn.to_owned()))
                    .filter(|child| self.owns_native_root(&child.root, &child.instance))
                    .map(|child| child.binding.clone())
            })
    }

    fn bind_descendant(
        &mut self,
        root: &CodexTurnBinding,
        instance: &str,
        request: &CodexApprovalRequest,
    ) -> Result<bool, DesktopError> {
        if request.thread_id.is_empty() || request.turn_id.is_empty() || request.item_id.is_empty()
        {
            return Ok(false);
        }
        if !self.owns_native_root(root, instance) {
            return Ok(false);
        }
        let key = (request.thread_id.clone(), request.turn_id.clone());
        if let Some(existing) = self.codex_descendants.get(&key) {
            return Ok(existing.root == *root && existing.instance == instance);
        }
        // An independently managed local root must never become somebody else's child.
        if self
            .codex_turn_links
            .values()
            .any(|binding| binding.codex_thread_id == request.thread_id)
        {
            return Ok(false);
        }
        self.storage.append_event(NewEvent {
            event_id: Uuid::new_v4().to_string(), task_id: root.task_id.clone(), turn_id: Some(root.turn_id.clone()),
            event_type: "codex_descendant_bound".into(),
            payload: json!({"root_thread_id":root.codex_thread_id,"root_turn_id":root.codex_turn_id,"child_thread_id":request.thread_id,"child_turn_id":request.turn_id}),
            created_at_ms: unix_time_ms()?,
        })?;
        self.codex_descendants.insert(
            key,
            Descendant {
                root: root.clone(),
                instance: instance.into(),
                binding: CodexTurnBinding {
                    task_id: root.task_id.clone(),
                    turn_id: root.turn_id.clone(),
                    codex_thread_id: request.thread_id.clone(),
                    codex_turn_id: request.turn_id.clone(),
                },
            },
        );
        Ok(true)
    }

    pub(super) fn clear_descendants_for_root(&mut self, turn: &str) {
        self.codex_descendants
            .retain(|_, child| child.root.turn_id != turn);
    }
}

pub(super) async fn prepare_approval(
    runtime: &Arc<Mutex<DesktopRuntime>>,
    client: &CodexKernelClient,
    request: &CodexApprovalRequest,
) -> Result<Option<String>, DesktopError> {
    let candidates = {
        let mut runtime = runtime.lock().map_err(|_| DesktopError::StateUnavailable)?;
        if let Some(root) = runtime.codex_turn_links.get(&request.turn_id).cloned() {
            if root.codex_thread_id != request.thread_id
                || !runtime.owns_native_root(&root, client.instance_id())
            {
                return Ok(None);
            }
            return runtime.prepare_codex_approval(request);
        }
        runtime
            .codex_turn_links
            .values()
            .filter(|root| runtime.owns_native_root(root, client.instance_id()))
            .cloned()
            .collect::<Vec<_>>()
    };
    if candidates.is_empty() {
        return Ok(None);
    }
    let roots = candidates
        .iter()
        .map(|root| root.codex_thread_id.clone())
        .collect();
    let ancestor = tokio::time::timeout(
        Duration::from_secs(10),
        local_agent_model::find_codex_ancestor(client, &request.thread_id, &roots),
    )
    .await;
    let Ok(Ok(Some(ancestor))) = ancestor else {
        crate::logging::warn(
            "codex_approval_ancestry_unconfirmed",
            json!({"thread_id":request.thread_id,"turn_id":request.turn_id}),
        );
        return Ok(None);
    };
    let mut matching = candidates
        .iter()
        .filter(|root| root.codex_thread_id == ancestor);
    let Some(root) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() {
        return Ok(None);
    }
    let mut runtime = runtime.lock().map_err(|_| DesktopError::StateUnavailable)?;
    if !runtime.bind_descendant(root, client.instance_id(), request)? {
        return Ok(None);
    }
    runtime.prepare_codex_approval(request)
}

#[cfg(test)]
#[path = "codex_descendants_tests.rs"]
mod tests;
