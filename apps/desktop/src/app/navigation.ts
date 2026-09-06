import type { ProjectSummary, TaskSummary, TimelineEntry } from "../bridge/types";

export function findTasks(
  tasks: TaskSummary[],
  timeline: TimelineEntry[],
  query: string,
  projects: ProjectSummary[] = [],
): TaskSummary[] {
  const needle = query.trim().toLocaleLowerCase();
  const matchingTasks = new Set<string>();
  const matchingProjects = new Set<string>();
  if (needle) {
    for (const entry of timeline) {
      if (`${entry.title}\n${entry.detail ?? ""}`.toLocaleLowerCase().includes(needle)) {
        matchingTasks.add(entry.taskId);
      }
    }
    for (const project of projects) {
      // Search the user-visible name, not machine-specific filesystem paths.
      if (project.name.toLocaleLowerCase().includes(needle)) matchingProjects.add(project.id);
    }
  }
  return tasks.filter((task) => !needle
    || matchingTasks.has(task.id)
    || matchingProjects.has(task.projectId)
    || `${task.title}\n${task.goal}`.toLocaleLowerCase().includes(needle))
    .sort((a, b) => Date.parse(b.updatedAt) - Date.parse(a.updatedAt));
}

export function relativeTaskTime(date: string, now = Date.now()): string {
  const minutes = Math.max(0, Math.floor((now - Date.parse(date)) / 60_000));
  if (!Number.isFinite(minutes)) return "";
  if (minutes < 1) return "刚刚";
  if (minutes < 60) return `${minutes} 分钟`;
  if (minutes < 1440) return `${Math.floor(minutes / 60)} 小时`;
  return `${Math.floor(minutes / 1440)} 天`;
}

export function draftKey(projectId: string | null | undefined, taskId?: string | null): string {
  return `${projectId ?? "none"}:${taskId ?? "new"}`;
}
