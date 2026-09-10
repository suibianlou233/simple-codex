import { test, expect } from "@playwright/test";

test("operation errors remain visible without exposing diagnostic details in the finished conversation", async ({page}, info) => {
  await page.goto("/tests/ui/fixture.html?scenario=operation-results");
  await expect(page.getByRole("alert")).toContainText("有操作结果需要检查");
  await expect(page.getByText("后续尝试可能已解决问题", {exact:false})).toBeVisible();
  await expect(page.locator(".chat-assistant")).toContainText("切换任务后");
  await expect(page.locator("body")).not.toContainText("PRIVATE_");
  expect(await page.locator(".work-process-dot").evaluate((element) => getComputedStyle(element).animationName)).toBe("none");
  await page.reload();
  await expect(page.getByRole("alert")).toContainText("有操作结果需要检查");
  await page.screenshot({path:info.outputPath("operation-results.png")});
});

test("terminal status stops animation despite stale action records and keeps failed attempt evidence after retry", async ({page}) => {
  await page.goto("/tests/ui/fixture.html?scenario=operation-transitions");
  await expect(page.getByRole("status")).toContainText("正在处理任务");
  expect(await page.locator(".work-process-dot").evaluate((element) => getComputedStyle(element).animationName)).toBe("gentle-pulse");
  await page.getByRole("button", {name:"模拟主轮完成",exact:true}).click();
  await expect(page.getByRole("alert")).toContainText("有操作结果需要检查");
  await expect(page.getByText("本轮已结束，但部分操作尚无最终结果记录。", {exact:false})).toBeVisible();
  await expect(page.locator(".chat-assistant")).toContainText("请验证实际效果");
  expect(await page.locator(".work-process-dot").evaluate((element) => getComputedStyle(element).animationName)).toBe("none");
  await page.getByRole("button", {name:"模拟补齐操作记录",exact:true}).click();
  await expect(page.getByText("本轮已结束，但部分操作尚无最终结果记录。", {exact:false})).toHaveCount(0);
  await expect(page.getByRole("alert")).toContainText("有操作结果需要检查");
  await expect(page.locator("body")).not.toContainText("这次任务未能完成");
  await expect(page.locator("body")).not.toContainText("PRIVATE_");
});

test("child results retain uncertainty beneath a completed root and expand only on request", async ({ page }, info) => {
  await page.setViewportSize({width:1280,height:1200});
  await page.goto("/tests/ui/fixture.html?scenario=child-results");
  await expect(page.getByRole("alert")).toContainText("有子任务结果需要检查");
  const results = page.locator("details.subagent-results");
  await expect(results).not.toHaveAttribute("open", "");
  await results.locator("summary").click();
  await expect(results).toContainText("修复任务搜索的中文输入法组合态");
  await expect(results).toContainText("已派发");
  await expect(results).toContainText("未能派发");
  await expect(results.locator("summary")).toContainText("3 次派发");
  await expect(results).toContainText("子任务 1：失败");
  await expect(results).toContainText("子任务 2：结果未知");
  await expect(results).toContainText("1 项子任务操作未获批准");
  await expect(page.locator("body")).not.toContainText("hidden-child");
  await expect(page.locator("body")).not.toContainText("hidden-parent");
  await page.screenshot({path:info.outputPath("child-results.png")});
  await page.locator(".task-row").filter({hasText:"整理项目启动文档"}).click();
  await expect(page.locator("details.subagent-results")).toHaveCount(0);
  await expect(page.getByText("有子任务结果需要检查", {exact:false})).toHaveCount(0);
  await page.locator(".task-row").filter({hasText:"让任务切换更加顺手"}).click();
  await expect(results).not.toHaveAttribute("open", "");
  await expect(page.getByRole("alert")).toContainText("有子任务结果需要检查");
});

test("completion waits remain active and keep stop available without claiming success", async ({ page }, info) => {
  for (const scenario of ["completion-wait", "completion-unknown"]) {
    await page.goto(`/tests/ui/fixture.html?scenario=${scenario}`);
    await expect(page.locator(".turn-status")).toHaveCount(0);
    await expect(page.getByRole("button", { name: "停止回复", exact: true })).toBeEnabled();
    await expect(page.getByRole("textbox", { name: "给 Simple 的任务" })).toBeDisabled();
    await expect(page.locator("body")).not.toContainText("处理已结束");
  }
  await page.screenshot({ path: info.outputPath("completion-uncertain.png") });
});

test("unknown submission keeps the composer blocked and explains that execution is not confirmed", async ({ page }, info) => {
  for (const scenario of ["submission-unknown", "submission-cold"]) {
    await page.goto(`/tests/ui/fixture.html?scenario=${scenario}`);
    await expect(page.locator(".turn-status")).toHaveCount(0);
    await expect(page.getByRole("textbox", {name:"给 Simple 的任务"})).toBeDisabled();
    await expect(page.locator("body")).not.toContainText("处理已结束");
    await expect(page.locator(".composer-shell")).not.toContainText("任务失败");
    await expect(page.locator(".work-process")).toHaveCount(0);
    await expect(page.getByRole("button", { name: "停止回复", exact: true })).toBeEnabled();
    await expect(page.getByRole("button", {name:"撤销修改", exact:true})).toHaveCount(0);
    await expect(page.locator("body")).not.toContainText("正在处理任务");
  }
  await page.screenshot({path:info.outputPath("submission-uncertain.png")});
});

test("workspace tools no longer expose file or diff panels", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("simple.ui.inspector", "diff"));
  await page.goto("/tests/ui/fixture.html");
  const tools = page.getByRole("navigation", { name: "工作区工具" });
  await expect(tools.getByRole("button")).toHaveCount(2);
  await expect(tools.getByRole("button", { name: "终端", exact:true })).toBeVisible();
  await expect(tools.getByRole("button", { name: "浏览器", exact:true })).toBeVisible();
  await expect(page.locator(".inspector-panel")).toHaveCount(0);
});

test("native final answer streams beside commentary before completion", async ({ page }) => {
  await page.goto("/tests/ui/fixture.html?scenario=commentary");
  await page.getByRole("button", {name:"模拟开始最终回复"}).click();
  await expect(page.getByText("修改完成，", {exact:true})).toBeVisible();
  await expect(page.locator(".work-process")).toContainText("正在处理任务");
  await expect(page.getByText("正在读取相关文件。", {exact:true})).toBeVisible();
  await page.getByRole("button", {name:"模拟完成本轮"}).click();
  await expect(page.getByText("修改完成，尚未验证。", {exact:true})).toBeVisible();
  await expect(page.locator(".completed-commentary > summary")).toContainText("执行过程 · 2 条说明");
  await expect(page.locator(".chat-assistant")).toHaveCount(3);
  await expect(page.locator(".chat-assistant:visible")).toHaveCount(1);
  await expect(page.locator(".work-process")).toHaveCount(0);
});

test.beforeEach(async ({ page }) => {
  await page.route("**/*", (route) => new URL(route.request().url()).hostname === "127.0.0.1" ? route.continue() : route.abort());
});

test("users can expand completed commentary without obscuring the final reply", async ({ page }) => {
  await page.goto("/tests/ui/fixture.html?scenario=commentary");
  await expect(page.getByText("正在读取相关文件。", { exact: true })).toBeVisible();
  await expect(page.getByText("正在更新输入法处理。", { exact: true })).toBeVisible();
  await expect(page.getByRole("status")).toContainText("正在处理任务");
  await expect(page.locator(".completed-commentary")).toHaveCount(0);
  await page.getByRole("button", { name: "模拟完成本轮" }).click();
  await expect(page.getByText("修改完成，尚未验证。", { exact: true })).toBeVisible();
  await expect(page.getByText("正在读取相关文件。", { exact: true })).not.toBeVisible();
  await expect(page.locator(".completed-commentary > summary")).toContainText("执行过程 · 2 条说明");
  await page.locator(".completed-commentary > summary").click();
  await expect(page.getByText("正在读取相关文件。", { exact: true })).toBeVisible();
  await expect(page.getByText("正在更新输入法处理。", { exact: true })).toBeVisible();
  await page.locator(".completed-commentary > summary").click();
  await expect(page.getByText("正在读取相关文件。", { exact: true })).not.toBeVisible();
  await expect(page.getByText("修改完成，尚未验证。", { exact: true })).toBeVisible();
  await expect(page.locator(".work-process")).toHaveCount(0);
});

test("welcome, suggestion, keyboard send and theme", async ({ page }, info) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/tests/ui/fixture.html?scenario=welcome");
  await expect(page.getByRole("heading", { name: "今天，想做点什么？" })).toBeVisible();
  await page.screenshot({ path: info.outputPath("welcome-light.png") });
  await page.getByRole("button", { name: /理解项目/ }).click();
  const input = page.getByRole("textbox", { name: "给 Simple 的任务" });
  await expect(input).toHaveValue(/先阅读项目/);
  await input.fill("界面验收：仅模拟发送");
  await input.press("Shift+Enter");
  await expect(input).toHaveValue("界面验收：仅模拟发送\n");
  await input.press("Enter");
  await expect(page.getByText("浏览器预览模式不会调用真实模型；桌面应用会在这里流式显示回复。", { exact: true })).toBeVisible();
  await expect(input).toHaveValue("");
  await page.getByRole("button", { name: /切换到深色主题/ }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  expect(errors).toEqual([]);
});

test("task search, independent drafts and collapsed sidebar", async ({ page }) => {
  await page.goto("/tests/ui/fixture.html");
  const input = page.getByRole("textbox", { name: "给 Simple 的任务" });
  await input.fill("任务 A 尚未发送");
  await page.locator(".task-row").filter({ hasText: "整理项目启动文档" }).click();
  await expect(input).toHaveValue("");
  await input.fill("任务 B 尚未发送");
  await page.keyboard.press("Control+k");
  await page.getByRole("textbox", { name: "搜索本地对话" }).fill("更加顺手");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(input).toHaveValue("任务 A 尚未发送");
  await page.keyboard.press("Control+b");
  await expect(page.getByRole("complementary", { name: "项目与任务" })).toHaveCount(0);
  await page.getByRole("button", { name: "展开侧栏" }).click();
  await expect(page.getByRole("complementary", { name: "项目与任务" })).toBeVisible();
  await page.keyboard.press("Control+n");
  await expect(input).toHaveValue("");
  await input.fill("新任务独立草稿");
  await page.locator(".task-row").filter({ hasText: "整理项目启动文档" }).click();
  await expect(input).toHaveValue("任务 B 尚未发送");
});

test("searching a project name opens its tasks across projects without sending a draft", async ({ page }, info) => {
  await page.goto("/tests/ui/fixture.html");
  const input = page.getByRole("textbox", { name: "给 Simple 的任务" });
  await input.fill("当前项目未发送的草稿");
  await page.keyboard.press("Control+k");
  const search = page.getByRole("textbox", { name: "搜索本地对话" });
  await search.fill("  SIMPLE-CODE  ");
  await expect(page.locator(".search-result")).toHaveCount(2);
  await search.fill("PLAYGROUND");
  await expect(page.locator(".search-result")).toHaveCount(1);
  await expect(page.locator(".search-result")).toContainText("检查配置读取的边界");
  await page.screenshot({ path: info.outputPath("project-name-search.png") });
  await search.press("Enter");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.locator(".workspace-heading")).toContainText("playground");
  await expect(input).toHaveValue("");
  await page.locator(".task-row").filter({ hasText: "让任务切换更加顺手" }).click();
  await expect(input).toHaveValue("当前项目未发送的草稿");
});

test("terminal uses an emulator instead of a command form", async ({ page }) => {
  await page.goto("/tests/ui/fixture.html");
  await page.getByRole("navigation", { name: "工作区工具" }).getByRole("button", { name: "终端", exact: true }).click();
  await expect(page.locator(".terminal-screen .xterm")).toBeVisible();
  await expect(page.locator(".terminal-panel form")).toHaveCount(0);
  await expect(page.getByRole("textbox", { name: "终端命令" })).toHaveCount(0);
  await page.locator(".task-row").filter({ hasText: "整理项目启动文档" }).click();
  await expect(page.locator(".terminal-slot:not([hidden]) .xterm")).toBeVisible();
  await expect(page.locator(".terminal-screen .xterm")).toHaveCount(2);
});

test("settings focus, escape and no horizontal overflow at minimum desktop size", async ({ page }, info) => {
  await page.setViewportSize({ width: 960, height: 640 });
  await page.goto("/tests/ui/fixture.html?scenario=long");
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.keyboard.press("Shift+Tab");
  await expect(page.getByRole("button", { name: /保存并使用/ })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(page.getByRole("dialog").getByRole("button", { name: "关闭", exact: true })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "设置", exact: true })).toBeFocused();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy();
  for (const code of await page.locator(".code-block").all()) {
    const bounds = await code.boundingBox();
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(960);
    await expect(code.getByRole("button", { name: "复制代码" })).toBeInViewport();
  }
  await page.screenshot({ path: info.outputPath("narrow-long-code.png") });
});

test("approval remains visible, reject does not apply, stop remains usable", async ({ page }, info) => {
  await page.setViewportSize({ width: 960, height: 640 });
  await page.goto("/tests/ui/fixture.html?scenario=approval");
  await expect(page.getByRole("button", { name: "应用修改", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "任务权限：逐项确认", exact: true })).toBeDisabled();
  await page.screenshot({ path: info.outputPath("pending-approval.png") });
  await page.getByRole("button", { name: "拒绝", exact: true }).click();
  await expect(page.getByRole("button", { name: "应用修改", exact: true })).toHaveCount(0);
  await expect(page.locator(".work-process")).toContainText("正在处理任务");
  await page.getByRole("button", { name: "停止回复", exact: true }).click();
  await expect(page.getByRole("textbox", { name: "给 Simple 的任务" })).toBeEnabled();
});

test("running commands and outputs do not enter user dialogue, stop remains available", async ({ page }) => {
  await page.goto("/tests/ui/fixture.html?scenario=running");
  await expect(page.locator(".work-process")).toContainText("正在处理任务");
  await expect(page.locator(".work-process-attention")).toHaveCount(0);
  await expect(page.getByText("Get-Content AGENTS.md", { exact: true })).not.toBeVisible();
  await expect(page.getByText("模拟命令输出", { exact: true })).not.toBeVisible();
  await expect(page.getByRole("button", { name: "停止回复", exact: true })).toBeVisible();
  await expect(page.getByText("Get-Content AGENTS.md", { exact: true })).toHaveCount(0);
  await expect(page.getByText("模拟命令输出", { exact: true })).toHaveCount(0);
  await expect(page.getByText("Get-Content AGENTS.md", { exact: true })).not.toBeVisible();
  await page.getByRole("button", { name: "停止回复", exact: true }).click();
  await expect(page.getByRole("textbox", { name: "给 Simple 的任务" })).toBeEnabled();
  await expect(page.getByText("Get-Content AGENTS.md", { exact: true })).not.toBeVisible();
});

test("approval exposes the proposed operation, not completed execution output", async ({ page }) => {
  await page.goto("/tests/ui/fixture.html?scenario=approval");
  await expect(page.locator(".approval-operation")).not.toHaveAttribute("open", "");
  await page.locator(".approval-operation > summary").click();
  await expect(page.locator(".approval-operation")).toContainText("src/components/Composer.tsx");
  await expect(page.locator(".approval-operation")).toContainText("+drafts[taskId]");
  await page.getByRole("button", { name: "应用修改", exact: true }).click();
  await expect(page.getByRole("button", { name: "应用修改", exact: true })).toHaveCount(0);
  await expect(page.getByText("操作已模拟执行", { exact: true })).toHaveCount(0);
});

test("permission escalation requires explicit confirmation and fits viewport", async ({ page }, info) => {
  await page.setViewportSize({ width: 960, height: 640 });
  await page.goto("/tests/ui/fixture.html?scenario=welcome");
  const trigger = page.getByRole("button", { name: "任务权限：逐项确认", exact: true });
  await trigger.click();
  await page.getByRole("radio", { name: /全机自动/ }).click();
  await expect(trigger).toHaveAttribute("aria-label", "任务权限：逐项确认");
  await expect(page.getByRole("button", { name: "确认开启" })).toBeInViewport();
  const bounds = await page.getByRole("dialog", { name: "选择任务权限" }).boundingBox();
  expect(bounds!.y).toBeGreaterThanOrEqual(0);
  expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(960);
  await page.screenshot({ path: info.outputPath("permission-confirmation.png") });
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await page.keyboard.press("Escape");
  await expect(trigger).toHaveAttribute("aria-label", "任务权限：逐项确认");
});

test("copy code works, clipboard unavailability has readable feedback", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto("/tests/ui/fixture.html");
  const copy = page.getByRole("button", { name: "复制代码", exact: true });
  await copy.click();
  await expect(copy).toContainText("已复制");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toContain("draftKey(projectId, taskId)");
  await page.evaluate(() => Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true }));
  await copy.click();
  await expect(copy).toContainText("复制失败，请手动选择");
});

test("attachment errors stay visible without discarding draft", async ({ page }) => {
  await page.goto("/tests/ui/fixture.html?scenario=attachment-error");
  await page.getByRole("textbox", { name: "给 Simple 的任务" }).fill("保留这个草稿");
  await page.getByRole("button", { name: "添加项目文件" }).click();
  await expect(page.getByRole("alert")).toContainText("未能添加附件");
  await expect(page.getByRole("alert")).not.toContainText("模拟附件读取失败");
  await expect(page.getByRole("textbox", { name: "给 Simple 的任务" })).toHaveValue("保留这个草稿");
});

test("attachment-only send survives model configuration", async ({ page }) => {
  await page.goto("/tests/ui/fixture.html?scenario=configure");
  await page.getByRole("button", { name: "添加项目文件" }).click();
  await page.getByRole("button", { name: "发送", exact: true }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.getByRole("button", { name: "兼容接口", exact: true }).click();
  await page.getByLabel("显示名称").fill("UI-only test");
  await page.getByLabel("接口地址").fill("http://127.0.0.1/unused");
  await page.getByLabel("模型名称").fill("mock");
  await page.getByRole("button", { name: /保存并使用/ }).click();
  await expect(page.locator(".chat-user")).toContainText("docs/ui-fixture.md");
  await expect(page.getByLabel("待发送附件")).toHaveCount(0);
});
