import { describe, it, expect } from "vitest";
import { draftKey, findTasks, relativeTaskTime } from "./navigation";
import type { ProjectSummary, TaskSummary } from "../bridge/types";

const tasks: TaskSummary[] = [
  { id: "old", projectId: "a", title: "修复输入", goal: "IME", status: "completed", permissionLevel: "approval", updatedAt: "2026-09-05T00:00:00Z" },
  { id: "new", projectId: "b", title: "阅读项目", goal: "架构", status: "ready", permissionLevel: "approval", updatedAt: "2026-09-05T01:00:00Z" },
];
describe("workspace navigation", () => {
  const projects: ProjectSummary[] = [
    { id: "a", name: "Simple-Code", path: "E:/private-path-only" },
    { id: "b", name: "中文项目", path: "E:/another-path" },
  ];
  it("finds every task in a named project, normalizing case and whitespace", () => {
    const sameProject = { ...tasks[0], id: "same-project", updatedAt: "2026-09-05T02:00:00Z" };
    expect(findTasks([...tasks, sameProject], [], "  SIMPLE-code  ", projects).map((task) => task.id)).toEqual(["same-project", "old"]);
    expect(findTasks(tasks, [], "中文", projects).map((task) => task.id)).toEqual(["new"]);
  });
  it("keeps original matching when project information is provided or missing", () => {
    expect(findTasks(tasks, [], "IME", projects).map((task) => task.id)).toEqual(["old"]);
    expect(findTasks(tasks, [], "修复", []).map((task) => task.id)).toEqual(["old"]);
    expect(findTasks(tasks, [], "Simple-Code", []).map((task) => task.id)).toEqual([]);
  });
  it("does not index project paths or duplicate tasks matching multiple fields", () => {
    expect(findTasks(tasks, [], "private-path-only", projects)).toEqual([]);
    expect(findTasks([{ ...tasks[0], title: "Simple-Code" }], [], "simple-code", projects).map((task) => task.id)).toEqual(["old"]);
  });
  it("keeps blank query ordering and leaves input arrays unchanged", () => {
    const taskOrder = tasks.map((task) => task.id);
    const projectOrder = projects.map((project) => project.id);
    expect(findTasks(tasks, [], " \n ", projects).map((task) => task.id)).toEqual(["new", "old"]);
    expect(tasks.map((task) => task.id)).toEqual(taskOrder);
    expect(projects.map((project) => project.id)).toEqual(projectOrder);
  });
  it("searches across projects by title, goal and local message content", () => {
    expect(findTasks(tasks, [], "ime").map((task) => task.id)).toEqual(["old"]);
    expect(findTasks(tasks, [{ id: "msg", taskId: "new", kind: "assistant", title: "", detail: "SQLite 持久化", createdAt: "" }], "sqlite").map((task) => task.id)).toEqual(["new"]);
  });
  it("sorts recent tasks without mutating the projection", () => {
    expect(findTasks(tasks, [], "").map((task) => task.id)).toEqual(["new", "old"]);
    expect(tasks[0].id).toBe("old");
  });
  it("keeps new-task and per-project drafts separate", () => {
    expect(new Set([draftKey("a"), draftKey("b"), draftKey("a", "old")]).size).toBe(3);
  });
  it("handles future and malformed timestamps without broken labels", () => {
    expect(relativeTaskTime("bad", 0)).toBe("");
    expect(relativeTaskTime("2026-09-05T01:00:00Z", Date.parse("2026-09-05T00:00:00Z"))).toBe("刚刚");
    expect(relativeTaskTime("2026-09-05T00:00:00Z", Date.parse("2026-09-05T02:00:00Z"))).toBe("2 小时");
  });
});
