import { test, expect } from "@playwright/test";

test("confirmed project memory can be edited, refreshed and deleted only after confirmation", async ({page}, info) => {
  await page.goto("/tests/ui/memory-fixture.html");
  const editor = page.getByRole("region", {name:"用户确认的项目记忆"});
  await editor.locator("summary").click();
  const input = editor.getByRole("textbox", {name:"确认记忆内容"});
  await expect(input).toHaveValue("A 的已确认约定");
  await input.fill("使用 pnpm，不是 npm\n<script>not executable</script>");
  await editor.getByRole("button", {name:"保存确认记忆"}).click();
  await expect(editor.getByRole("status")).toContainText("已保存");
  await editor.getByRole("button", {name:"加载最新版本"}).click();
  await expect(input).toHaveValue("使用 pnpm，不是 npm\n<script>not executable</script>");
  await page.screenshot({path:info.outputPath("manual-project-memory-saved.png")});
  await editor.getByRole("button", {name:"删除确认记忆"}).click();
  await editor.getByRole("button", {name:"取消",exact:true}).click();
  await expect(input).not.toHaveValue("");
  await editor.getByRole("button", {name:"删除确认记忆"}).click();
  await editor.getByRole("button", {name:"确认删除"}).click();
  await expect(input).toHaveValue("");
  await expect(editor.getByRole("status")).toContainText("旧对话和自动摘要未被删除");
  await expect(editor.locator("script")).toHaveCount(0);
  await page.screenshot({path:info.outputPath("manual-project-memory.png")});
});

test("conflicting memory save keeps the draft and requires confirmation before replacing it", async ({page}) => {
  await page.goto("/tests/ui/memory-fixture.html?scenario=notes-conflict");
  const editor = page.getByRole("region", {name:"用户确认的项目记忆"});
  await editor.locator("summary").click();
  const input = editor.getByRole("textbox", {name:"确认记忆内容"});
  await input.fill("我的未保存纠正");
  await editor.getByRole("button", {name:"保存确认记忆"}).click();
  await expect(editor.getByRole("alert")).toContainText("编辑内容已保留");
  await expect(input).toHaveValue("我的未保存纠正");
  await expect(page.locator("body")).not.toContainText("PRIVATE_BACKEND_CONFLICT");
  await editor.getByRole("button", {name:"加载最新版本"}).click();
  await editor.getByRole("button", {name:"取消",exact:true}).click();
  await expect(input).toHaveValue("我的未保存纠正");
  await editor.getByRole("button", {name:"加载最新版本"}).click();
  await editor.getByRole("button", {name:"确认加载"}).click();
  await expect(input).toHaveValue("另一个窗口的最新纠正");
});

test("late memory save cannot put project A content or success feedback into project B", async ({page}) => {
  await page.goto("/tests/ui/memory-fixture.html?scenario=notes-save-race");
  const editor = page.getByRole("region", {name:"用户确认的项目记忆"});
  await editor.locator("summary").click();
  await editor.getByRole("textbox", {name:"确认记忆内容"}).fill("A_SAVED_LATE");
  await editor.getByRole("button", {name:"保存确认记忆"}).click();
  await expect(editor.getByRole("status")).toContainText("正在保存");
  await page.getByRole("button", {name:"切换项目 B"}).click();
  await editor.locator("summary").click();
  await expect(editor.getByRole("textbox")).toHaveValue("B 的独立约定");
  await page.getByRole("button", {name:"返回旧项目结果"}).click();
  await expect(editor.getByRole("textbox")).toHaveValue("B 的独立约定");
  await expect(editor.getByText("已保存", {exact:false})).toHaveCount(0);
});

test("oversized confirmed memory remains editable but cannot be submitted", async ({page}) => {
  await page.goto("/tests/ui/memory-fixture.html");
  const editor = page.getByRole("region", {name:"用户确认的项目记忆"});
  await editor.locator("summary").click();
  await editor.getByRole("textbox").fill("中".repeat(3000));
  await expect(editor.getByRole("alert")).toContainText("超过 8 KB");
  await expect(editor.getByRole("button", {name:"保存确认记忆"})).toBeDisabled();
  await expect(editor.getByRole("textbox")).toBeEnabled();
});

test("source exclusion requires confirmation and explains preserved history", async ({ page }, info) => {
  await page.goto("/tests/ui/memory-fixture.html?scenario=forget");
  await page.getByText("长期记忆索引", { exact: false }).click();
  const forget = page.getByRole("button", { name: "清空并停止旧对话学习" });
  page.once("dialog", async (dialog) => {
    expect(dialog.message()).toContain("不会删除聊天记录");
    await dialog.dismiss();
  });
  await forget.click();
  await expect(page.locator("pre")).toContainText("记忆-A");
  page.once("dialog", (dialog) => dialog.accept());
  await forget.click();
  await expect(page.getByRole("status").filter({ hasText: "原聊天记录仍然保留" })).toBeVisible();
  await expect(page.locator("pre")).toHaveCount(0);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy();
  await page.screenshot({ path: info.outputPath("memory-source-exclusion.png") });
});

test("busy memory reset retains content and a confirmed retry refreshes after success", async ({ page }) => {
  await page.goto("/tests/ui/memory-fixture.html?scenario=reset-busy");
  await page.getByText("长期记忆索引", { exact: false }).click();
  await expect(page.locator("pre")).toContainText("记忆-A");
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "清空自动记忆", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("正在整理，尚未清空");
  await expect(page.locator("body")).not.toContainText("C:/private");
  await expect(page.locator("pre")).toContainText("记忆-A");
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "清空自动记忆", exact: true }).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.locator("pre")).toHaveCount(0);
  const memory = page.locator("details").filter({ hasText: "长期记忆索引" });
  if (await memory.getAttribute("open") === null) await memory.locator("summary").click();
  await expect(memory.getByText("尚未生成此项记忆")).toBeVisible();
});

test("memory viewer collapses content, escapes HTML and distinguishes missing from failure", async ({ page }, info) => {
  await page.goto("/tests/ui/memory-fixture.html");
  await expect(page.getByText("这是旧会话使用的独立记忆区", { exact: false })).toBeVisible();
  await expect(page.locator("pre")).not.toBeVisible();
  await page.getByText("长期记忆索引", { exact: false }).click();
  await expect(page.locator("pre")).toContainText("<script>");
  await expect(page.locator("pre script")).toHaveCount(0);
  await page.locator("summary").filter({ hasText: "摘要" }).click();
  await expect(page.getByText("尚未生成此项记忆")).toBeVisible();
  await page.getByText("来源记录", { exact: false }).click();
  await expect(page.getByText("无法读取此项记忆", { exact: false })).toBeVisible();
  await page.screenshot({ path: info.outputPath("project-memory.png") });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy();
});

test("late memory response never replaces the newly selected project", async ({ page }) => {
  await page.goto("/tests/ui/memory-fixture.html?scenario=race");
  await expect(page.getByRole("status")).toContainText("正在读取");
  await page.getByRole("button", { name: "切换项目 B" }).click();
  await expect(page.getByText("所属项目：E:/项目-B")).toBeVisible();
  await page.getByRole("button", { name: "返回旧项目结果" }).click();
  await page.getByText("长期记忆索引", { exact: false }).click();
  await expect(page.locator("pre")).toContainText("记忆-B");
  await expect(page.locator("body")).not.toContainText("记忆-A");
});

test("memory read failure is not empty success and refresh can recover", async ({ page }) => {
  await page.goto("/tests/ui/memory-fixture.html?scenario=failure");
  await expect(page.getByRole("alert")).toContainText("这不表示记忆为空");
  await expect(page.locator("body")).not.toContainText("INTERNAL_SECRET_FIXTURE");
  await page.getByRole("button", { name: "刷新" }).click();
  await expect(page.getByText("所属项目：E:/项目-A")).toBeVisible();
});

test("memory remains readable when native capabilities fail to load", async ({ page }) => {
  await page.goto("/tests/ui/memory-fixture.html?scenario=kernel-failure");
  await expect(page.getByRole("dialog")).toBeVisible();
  await expect(page.getByText("暂时无法读取能力")).toBeVisible();
  await expect(page.getByText("所属项目：E:/项目-A")).toBeVisible();
  await page.getByText("长期记忆索引", { exact: false }).click();
  await expect(page.locator("pre")).toContainText("记忆-A");
});

test("a mismatched task response is never displayed", async ({ page }) => {
  await page.goto("/tests/ui/memory-fixture.html?scenario=wrong-task");
  await expect(page.getByRole("alert")).toContainText("这不表示记忆为空");
  await expect(page.locator("body")).not.toContainText("项目-OTHER");
});

test("switching projects collapses previously expanded memory", async ({ page }) => {
  await page.goto("/tests/ui/memory-fixture.html");
  await page.getByText("长期记忆索引", { exact: false }).click();
  await expect(page.locator("pre")).toContainText("记忆-A");
  await page.getByRole("button", { name: "切换项目 B" }).click();
  await expect(page.getByText("所属项目：E:/项目-B")).toBeVisible();
  await expect(page.locator("pre")).not.toBeVisible();
  await page.getByText("长期记忆索引", { exact: false }).click();
  await expect(page.locator("pre")).toContainText("记忆-B");
});
