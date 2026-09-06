// Public assembly API; views consume only the typed local bridge.
export { App, WorkspaceNavigation, permissionForNextConversation } from "./app/WorkbenchApp";
export { AgentSettings } from "./settings/AgentSettings";
export { ModelSettings } from "./settings/ModelSettings";
export { Composer } from "./components/Composer";
export { PermissionSelector } from "./components/PermissionSelector";
export { EmptyWorkspace } from "./components/Welcome";
export { ThemePicker } from "./components/ThemePicker";
export { TaskTimeline, MarkdownMessage, summarizeWorkProcess } from "./items/TaskTimeline";
export { toMessage } from "./app/feedback";
