import { Icon, SimpleMark, type IconName } from "./Icon";

const suggestions: Array<{ icon: IconName; title: string; detail: string; prompt: string }> = [
  { icon: "code", title: "理解项目", detail: "梳理结构与关键调用链", prompt: "先阅读项目的 AGENTS.md 和 README，再梳理目录结构与关键调用链。告诉我项目如何运行，不要修改文件。" },
  { icon: "diff", title: "改进代码", detail: "从一个真实问题开始", prompt: "阅读 AGENTS.md 和 DEBUG.md，检查当前项目，找一个真实且范围较小的问题。先说明证据、修改方案和涉及文件，等我确认后再修改。" },
  { icon: "review", title: "检查改动", detail: "发现风险，补足验证", prompt: "检查工作区当前改动，优先找出可能影响实际使用的缺陷。说明文件位置、原因和验证方式；只读检查，不修改代码。" },
];

export function EmptyWorkspace({ hasProject, projectName, onOpenProject, onSuggestion }: { hasProject: boolean; projectName?: string; onOpenProject: () => void; onSuggestion?: (prompt: string) => void }) {
  return <div className="empty-workspace">
    <SimpleMark large />
    <p className="welcome-kicker">你的代码。你的模型。你的工作台。</p>
    <h1>{hasProject ? "今天，想做点什么？" : "从你的代码开始。"}</h1>
    <p className="welcome-description">{hasProject ? `在 ${projectName ?? "当前项目"} 中理解、构建与改进。把下一步交给 Simple。` : "打开一个本地项目，接入自己的模型，让想法成为代码。"}</p>
    {hasProject ? <div className="suggestion-grid">{suggestions.map((item) => <button key={item.title} onClick={() => onSuggestion?.(item.prompt)}><Icon name={item.icon} size={20} /><strong>{item.title}</strong><small>{item.detail}</small><span className="suggestion-arrow">↗</span></button>)}</div> : <button className="primary-action" onClick={onOpenProject}><Icon name="folder" size={17} />打开本地项目</button>}
    {!hasProject ? <small className="welcome-privacy">无需厂商账号 · 本地保存记录 · 模型调用由你配置</small> : null}
  </div>;
}
