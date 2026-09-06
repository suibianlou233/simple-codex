import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import type {
  ContextUsage,
  DesktopBridge,
  DesktopSnapshot,
  ModelProfileSummary,
  ProjectSummary,
  RuntimeStatus,
  TaskSummary,
  TimelineEntry,
  ToolActionSummary,
  TurnSummary,
  Unsubscribe,
} from "../bridge/types";

type EntityState = {
  projectsById: Map<string, ProjectSummary>;
  projectOrder: string[];
  threadsById: Map<string, TaskSummary>;
  threadOrderByProject: Map<string, string[]>;
  turnsById: Map<string, TurnSummary>;
  turnOrderByThread: Map<string, string[]>;
  itemsById: Map<string, TimelineEntry>;
  itemOrderByThread: Map<string, string[]>;
  actionsById: Map<string, ToolActionSummary>;
  actionOrderByThread: Map<string, string[]>;
  modelProfiles: ModelProfileSummary[];
  contextUsage: ContextUsage[];
  activeModelProfileId: string | null;
  activeProjectId: string | null;
  activeTaskId: string | null;
  activeTurnId: string | null;
  taskRevisions: Record<string, number>;
  runtime: RuntimeStatus;
};

const EMPTY_RUNTIME: RuntimeStatus = {
  core: "unavailable",
  storage: "unavailable",
  network: "offline",
  dataLocation: "",
  workspaceSandboxReady: false,
  workspaceSandboxHealth: { status: "unsupported_platform" },
};

const emptyState = (): EntityState => ({
  projectsById: new Map(),
  projectOrder: [],
  threadsById: new Map(),
  threadOrderByProject: new Map(),
  turnsById: new Map(),
  turnOrderByThread: new Map(),
  itemsById: new Map(),
  itemOrderByThread: new Map(),
  actionsById: new Map(),
  actionOrderByThread: new Map(),
  modelProfiles: [],
  contextUsage: [],
  activeModelProfileId: null,
  activeProjectId: null,
  activeTaskId: null,
  activeTurnId: null,
  taskRevisions: {},
  runtime: EMPTY_RUNTIME,
});

function sameEntity<T>(left: T | undefined, right: T): boolean {
  return left !== undefined && JSON.stringify(left) === JSON.stringify(right);
}

const actionStatusRank: Record<ToolActionSummary["status"], number> = {
  pending: 0,
  running: 1,
  applied: 2,
  rejected: 2,
  failed: 2,
  undone: 3,
};

function mergeEntityMap<T extends { id: string }>(
  current: Map<string, T>,
  incoming: T[],
  accept?: (current: T | undefined, incoming: T) => boolean,
): Map<string, T> {
  const next = new Map(current);
  const live = new Set<string>();
  for (const entity of incoming) {
    live.add(entity.id);
    const existing = current.get(entity.id);
    if ((accept?.(existing, entity) ?? true) && !sameEntity(existing, entity)) {
      next.set(entity.id, entity);
    }
  }
  for (const id of next.keys()) if (!live.has(id)) next.delete(id);
  return next;
}

function groupOrder<T extends { id: string }>(
  values: T[],
  group: (value: T) => string,
): Map<string, string[]> {
  const result = new Map<string, string[]>();
  for (const value of values) {
    const key = group(value);
    const ids = result.get(key) ?? [];
    ids.push(value.id);
    result.set(key, ids);
  }
  return result;
}

export function applySnapshot(state: EntityState, snapshot: DesktopSnapshot): EntityState {
  const staleTaskIds = new Set(
    snapshot.tasks
      .filter((task) => (snapshot.taskRevisions[task.id] ?? 0) < (state.taskRevisions[task.id] ?? 0))
      .map((task) => task.id),
  );
  const effectiveTasks = snapshot.tasks.map((task) =>
    staleTaskIds.has(task.id) ? state.threadsById.get(task.id) ?? task : task,
  );
  const retainStale = <T extends { id: string }>(
    incoming: T[],
    current: Map<string, T>,
    taskId: (value: T) => string,
  ): T[] => {
    return [
      ...incoming.filter((value) => !staleTaskIds.has(taskId(value))),
      ...[...current.values()].filter(
        (value) => staleTaskIds.has(taskId(value)),
      ),
      ...incoming.filter((value) => staleTaskIds.has(taskId(value)) && !current.has(value.id)),
    ];
  };
  const effectiveTurns = retainStale(snapshot.turns, state.turnsById, (turn) => turn.taskId);
  const effectiveItems = retainStale(snapshot.timeline, state.itemsById, (item) => item.taskId);
  const effectiveActions = retainStale(snapshot.actions, state.actionsById, (action) => action.taskId);
  const tasksById = new Map(effectiveTasks.map((task) => [task.id, task]));
  const turnsById = mergeEntityMap(state.turnsById, effectiveTurns, (current, incoming) =>
    !current || incoming.sequence >= current.sequence,
  );
  const activeTaskId = snapshot.activeTaskId && tasksById.has(snapshot.activeTaskId)
    ? snapshot.activeTaskId : null;
  const candidate = snapshot.activeTurnId ? turnsById.get(snapshot.activeTurnId) : undefined;
  // Derive the control state from the same revision-checked facts as the messages.
  // A just-accepted turn may not yet have a turn row; preserve that pending lock,
  // but never resurrect a known terminal turn or borrow another task's turn.
  const runningTurn = [...turnsById.values()]
    .filter((turn) => turn.taskId === activeTaskId && turn.status === "running")
    .sort((a, b) => a.startedAt.localeCompare(b.startedAt)).at(-1);
  const activeTurnId = candidate?.taskId === activeTaskId && candidate.status === "running"
    ? candidate.id
    : runningTurn?.id ?? (activeTaskId && !candidate && !staleTaskIds.has(activeTaskId)
      ? snapshot.activeTurnId : null);
  return {
    projectsById: mergeEntityMap(state.projectsById, snapshot.projects),
    projectOrder: snapshot.projects.map((project) => project.id),
    threadsById: mergeEntityMap(state.threadsById, effectiveTasks),
    threadOrderByProject: groupOrder(effectiveTasks, (task) => task.projectId),
    turnsById,
    turnOrderByThread: groupOrder(effectiveTurns, (turn) => turn.taskId),
    itemsById: mergeEntityMap(state.itemsById, effectiveItems, (current, incoming) => {
      if (!current) return true;
      if (current.status === "completed" && incoming.status === "streaming") return false;
      // Stream revision is a process-wide UI counter, while persisted revision
      // is the task journal sequence. They are not comparable. Once the task
      // snapshot has passed the stale-task guard, its completed item is canonical.
      if (current.status === "streaming" && incoming.status === "completed") return true;
      return (incoming.revision ?? incoming.sequence ?? 0) >=
        (current.revision ?? current.sequence ?? 0);
    }),
    itemOrderByThread: groupOrder(effectiveItems, (item) => item.taskId),
    actionsById: mergeEntityMap(state.actionsById, effectiveActions, (current, incoming) =>
      !current || actionStatusRank[incoming.status] >= actionStatusRank[current.status],
    ),
    actionOrderByThread: groupOrder(effectiveActions, (action) => action.taskId),
    modelProfiles: snapshot.modelProfiles,
    contextUsage: snapshot.contextUsage,
    activeModelProfileId: snapshot.activeModelProfileId,
    activeProjectId: snapshot.activeProjectId,
    activeTaskId,
    activeTurnId,
    taskRevisions: Object.fromEntries(
      [...new Set([...Object.keys(state.taskRevisions), ...Object.keys(snapshot.taskRevisions)])]
        .map((taskId) => [taskId, Math.max(state.taskRevisions[taskId] ?? 0, snapshot.taskRevisions[taskId] ?? 0)]),
    ),
    runtime: snapshot.runtime,
  };
}

function materialize(state: EntityState): DesktopSnapshot {
  const values = <T,>(ids: string[], entities: Map<string, T>): T[] =>
    ids.flatMap((id) => {
      const entity = entities.get(id);
      return entity ? [entity] : [];
    });
  return {
    projects: values(state.projectOrder, state.projectsById),
    tasks: state.projectOrder.flatMap((id) =>
      values(state.threadOrderByProject.get(id) ?? [], state.threadsById),
    ),
    turns: [...state.turnOrderByThread.values()].flatMap((ids) =>
      values(ids, state.turnsById),
    ),
    timeline: [...state.itemOrderByThread.values()].flatMap((ids) =>
      values(ids, state.itemsById),
    ),
    actions: [...state.actionOrderByThread.values()].flatMap((ids) =>
      values(ids, state.actionsById),
    ),
    modelProfiles: state.modelProfiles,
    contextUsage: state.contextUsage,
    activeModelProfileId: state.activeModelProfileId,
    activeProjectId: state.activeProjectId,
    activeTaskId: state.activeTaskId,
    activeTurnId: state.activeTurnId,
    taskRevisions: state.taskRevisions,
    runtime: state.runtime,
  };
}

export class WorkbenchStore {
  private state = emptyState();
  private snapshot: DesktopSnapshot | undefined;
  private readonly listeners = new Set<() => void>();

  getSnapshot = (): DesktopSnapshot | undefined => this.snapshot;

  subscribe = (listener: () => void): Unsubscribe => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  hydrate = (snapshot: DesktopSnapshot): void => {
    this.state = applySnapshot(this.state, snapshot);
    this.snapshot = materialize(this.state);
    for (const listener of this.listeners) listener();
  };
}

export function useDesktopProjection(bridge: DesktopBridge): {
  snapshot: DesktopSnapshot | undefined;
  error: string | undefined;
} {
  const store = useMemo(() => new WorkbenchStore(), [bridge]);
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
  const [error, setError] = useState<string>();

  useEffect(() => {
    let alive = true;
    const unsubscribe = bridge.subscribe((next) => {
      if (alive) store.hydrate(next);
    });
    void bridge.load().then(store.hydrate).catch((cause: unknown) => {
      setError(cause instanceof Error ? cause.message : String(cause));
    });
    return () => {
      alive = false;
      unsubscribe();
    };
  }, [bridge, store]);

  return { snapshot, error };
}
