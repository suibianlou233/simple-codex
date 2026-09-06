export function toMessage(cause: unknown): string {
  const raw = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "";
  // Only return curated copy. Never echo URLs, server bodies, paths or stacks.
  if (raw.includes("memory source exclusion saved; artifact cleanup incomplete"))
    return "旧对话的自动学习已停止，但记忆文件尚未完全清理。请稍后重试清空；不要将这次操作视为全部完成。";
  if (raw.includes("memory workspace busy; retry after memory processing finishes"))
    return "项目记忆正在整理，尚未清空。请稍后重试。";
  if (/401|403|unauthorized|invalid.api.key|密钥|凭据/i.test(raw))
    return "模型连接未获授权，请检查模型设置中的访问凭据。";
  if (/timeout|timed out|超时/i.test(raw))
    return "等待响应的时间较长，本次操作未完成。请稍后重试。";
  if (/502|503|504|network|fetch|connection|stream disconnected|连接|网络/i.test(raw))
    return "暂时无法连接模型服务，请检查网络或稍后重试。";
  if (/attachment|附件/i.test(raw))
    return "未能添加附件，请确认文件可用后重新选择。";
  if (/permission|access.denied|拒绝|权限/i.test(raw))
    return "当前权限不允许这项操作，请检查所选的任务权限。";
  return "这次操作未能完成，请稍后重试。";
}
