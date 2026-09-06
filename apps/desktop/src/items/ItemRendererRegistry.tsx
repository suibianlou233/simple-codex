import { MarkdownMessage } from "./MarkdownMessage";
import type { TimelineEntry } from "../bridge/types";
import { toMessage } from "../app/feedback";

export type ItemRendererProps = { item: TimelineEntry };
export type ItemRenderer = (props: ItemRendererProps) => React.ReactNode;

const plain = ({ item }: ItemRendererProps) => (
  <p className="message-text">{item.detail ?? item.title}</p>
);

const assistant = ({ item }: ItemRendererProps) => <MarkdownMessage content={item.detail ?? item.title} />;

const detail = ({ item }: ItemRendererProps) => (
  <details className={`structured-item structured-${item.kind}`} open={!item.lowValue}>
    <summary>
      <span>{item.title}</span>
      {item.status ? <small>{{ streaming: "生成中", pending: "等待确认", running: "运行中", completed: "已完成", failed: "失败", cancelled: "已停止" }[item.status]}</small> : null}
    </summary>
    {item.detail ? <pre>{item.detail}</pre> : null}
  </details>
);

const alert = ({ item }: ItemRendererProps) => (
  <div className="structured-alert" role="alert">
    <strong>这次操作未能完成</strong>
    <p>{toMessage(item.detail ?? item.title)}</p>
  </div>
);

export const itemRendererRegistry: Record<TimelineEntry["kind"], ItemRenderer> = {
  user: plain,
  assistant,
  command: detail,
  file_read: detail,
  file_change: detail,
  approval: detail,
  error: alert,
  lifecycle: detail,
};

export function ItemRendererView({ item }: ItemRendererProps) {
  const Renderer = itemRendererRegistry[item.kind];
  return <>{Renderer({ item })}</>;
}
