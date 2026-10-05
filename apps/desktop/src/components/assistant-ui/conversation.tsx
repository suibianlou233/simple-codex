import {createContext, useContext, useMemo, useRef, type PropsWithChildren, type ReactNode} from "react";
import {AssistantRuntimeProvider, useExternalStoreRuntime, MessageProvider, MessagePrimitive, ThreadPrimitive, type ThreadMessage} from "@assistant-ui/react";
import type {TimelineEntry} from "../../bridge/types";
import "./conversation.css";

type Actions = {send: () => Promise<void>; cancel: () => Promise<void>};
const MessageIndices = createContext<Map<string, number>>(new Map());
const ConversationContext = createContext<{current: Actions | null} | null>(null);
export function useConversationActions() {return useContext(ConversationContext);}
export function ConversationBoundary({entries = [], running = false, children}: PropsWithChildren<{entries?: TimelineEntry[]; running?: boolean}>) {
 const existing = useContext(ConversationContext);
 return existing ? children : <ConversationRuntime entries={entries} running={running}>{children}</ConversationRuntime>;
}
function ConversationRuntime({entries, running, children}: PropsWithChildren<{entries: TimelineEntry[]; running: boolean}>) {
 const actions = useRef<Actions | null>(null);
 const messages = useMemo(() => entries.filter(e => e.kind === "user" || e.kind === "assistant").map(toAssistantMessage), [entries]);
 const indices = useMemo(() => new Map(messages.map((m,i) => [m.id,i])), [messages]);
 const runtime = useExternalStoreRuntime({messages, isRunning: running,
   onNew: async () => {await actions.current?.send();},
   onCancel: async () => {await actions.current?.cancel();},
 });
 return <ConversationContext.Provider value={actions}><AssistantRuntimeProvider runtime={runtime}><MessageIndices.Provider value={indices}>{children}</MessageIndices.Provider></AssistantRuntimeProvider></ConversationContext.Provider>;
}
export function toAssistantMessage(entry: TimelineEntry): ThreadMessage {
 const common = {id: entry.id, createdAt: new Date(entry.createdAt), content: [{type: "text" as const, text: entry.detail ?? entry.title}], metadata: {custom: {}}};
 return entry.kind === "user" ? {...common, role: "user", attachments: []} : {...common, role: "assistant", metadata: {custom: {}, unstable_state: null, unstable_annotations: [], unstable_data: [], steps: []}, status: entry.status === "streaming" ? {type: "running"} : {type: "complete", reason: "stop"}};
}
export function ConversationMessage({entry, children}: {entry: TimelineEntry; children: ReactNode}) {
 const index = useContext(MessageIndices).get(entry.id);
 const message = useMemo(() => toAssistantMessage(entry), [entry]);
 if (entry.kind !== "user" && entry.kind !== "assistant") return <article className={`chat-message chat-${entry.kind}`}>{children}</article>;
 const body = <MessagePrimitive.Root asChild><article className={`chat-message chat-${entry.kind}${entry.phase === "commentary" ? " chat-commentary" : ""}`}>{children}</article></MessagePrimitive.Root>;
 return <MessageProvider message={message} index={index ?? 0}>{body}</MessageProvider>;
}
export {ThreadPrimitive};
