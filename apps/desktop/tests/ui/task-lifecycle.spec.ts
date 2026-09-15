import { test, expect } from "@playwright/test";

test("archive, restore, cancel deletion and delete a conversation", async ({ page }) => {
  await page.goto("/tests/ui/frontend-fixes.html?scenario=completed");
  const entry = page.locator(".task-list-entry").first();
  const title = await entry.locator(".task-title").textContent();
  expect(title).toBeTruthy();
  await entry.locator("summary").click();
  await page.screenshot({ path: "../../target/task-menu.png" });
  await entry.getByRole("button", { name: "归档对话", exact: true }).click();
  await expect(page.locator(".task-title").filter({ hasText: title! })).toHaveCount(0);
  await page.getByRole("button", { name: "已归档" }).click();
  await expect(page.locator(".task-title").filter({ hasText: title! })).toHaveCount(1);
  await page.locator(".task-actions summary").first().click();
  await page.getByRole("button", { name: "恢复对话", exact: true }).click();
  await page.getByRole("button", { name: "返回对话" }).click();
  await page.locator(".task-actions summary").first().click();
  await page.getByRole("button", { name: "删除对话", exact: true }).first().click();
  await page.screenshot({ path: "../../target/task-delete-dialog.png" });
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await expect(page.locator(".task-title").filter({ hasText: title! })).toHaveCount(1);
  await page.locator(".task-actions summary").first().click();
  await page.getByRole("button", { name: "删除对话", exact: true }).first().click();
  await page.getByRole("button", { name: "确认删除", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "删除对话" })).toHaveCount(0);
  await expect(page.locator(".task-title").filter({ hasText: title! })).toHaveCount(0);
});

test("running work disables archive and deletion", async ({ page }) => {
  await page.goto("/tests/ui/frontend-fixes.html?scenario=running");
  await page.locator(".task-actions summary").first().click();
  await expect(page.getByRole("button", { name: "归档对话", exact: true }).first()).toBeDisabled();
  await expect(page.getByRole("button", { name: "删除对话", exact: true }).first()).toBeDisabled();
});
