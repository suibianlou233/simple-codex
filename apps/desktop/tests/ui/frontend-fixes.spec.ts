import { test, expect } from "@playwright/test";

test("composer has no status strip while running and still offers stop", async ({ page }) => {
  await page.goto("/tests/ui/frontend-fixes.html?scenario=running");
  await expect(page.getByRole("textbox", {name:"给 Simple 的任务"})).toBeEnabled();
  await expect(page.getByRole("button", {name:"停止回复",exact:true})).toBeVisible();
  await expect(page.locator(".composer-shell .turn-status")).toHaveCount(0);
  await expect(page.locator(".composer-shell")).not.toContainText("正在处理任务");
});

test("omits the duplicate uncertain-status row but retains the send lock and stop control", async ({ page }) => {
  await page.goto("/tests/ui/frontend-fixes.html?scenario=uncertain");
  await expect(page.getByRole("textbox",{name:"给 Simple 的任务"})).toBeDisabled();
  await expect(page.getByRole("button",{name:"停止回复",exact:true})).toBeVisible();
  await expect(page.getByText("任务执行状态待确认，请勿重复发送",{exact:true})).toHaveCount(0);
  await expect(page.locator(".work-process-live")).toHaveCount(0);
});

test("failed edit keeps its draft and a successful retry closes the editor", async ({ page }) => {
  await page.goto("/tests/ui/frontend-fixes.html");
  await page.getByRole("button", {name:"编辑", exact:true}).click();
  await page.getByRole("textbox", {name:"编辑消息内容"}).fill("修改后的请求");
  page.on("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", {name:"重新发送", exact:true}).click();
  await expect(page.getByRole("textbox", {name:"编辑消息内容"})).toHaveValue("修改后的请求");
  await expect(page.getByText("未能重新发送，编辑内容已保留，请稍后重试。")).toBeVisible();
  await page.getByRole("button", {name:"重新发送", exact:true}).click();
  await expect(page.getByRole("textbox", {name:"编辑消息内容"})).toHaveCount(0);
});

for (const scenario of ["completed", "cancelled"]) test(`${scenario} notes fold together but terminal notice stays visible`, async ({ page }) => {
  await page.goto(`/tests/ui/frontend-fixes.html?scenario=${scenario}`);
  await expect(page.locator(".turn-execution")).toHaveCount(1);
  await expect(page.locator(".turn-execution")).not.toHaveAttribute("open", "");
  await expect(page.locator(".turn-execution > summary")).toContainText("用时 12秒");
  await expect(page.locator(".chat-message .turn-elapsed")).toHaveCount(0);
  await expect(page.getByText("第一条过程说明")).toBeHidden();
  if (scenario === "cancelled") await expect(page.getByText("任务已停止", {exact:true})).toBeVisible();
  else await expect(page.getByText("最终回答", {exact:true})).toBeVisible();
  await page.locator(".turn-execution > summary").click();
  await expect(page.getByText("第一条过程说明")).toBeVisible();
  await expect(page.getByText("第二条过程说明")).toBeVisible();
});
