import { test, expect } from "@playwright/test";

test("first-run guide dismisses once and remains available in settings", async ({ page }, info) => {
  await page.goto("/tests/ui/fixture.html?scenario=onboarding");
  const guide = page.getByRole("dialog", { name: "欢迎使用 Simple" });
  await expect(guide).toBeVisible();
  await expect(guide.getByRole("button", { name: "开始配置" })).toBeFocused();
  await page.keyboard.press("Control+k");
  await expect(page.getByRole("dialog")).toHaveCount(1);
  await page.keyboard.press("Shift+Tab");
  await expect(guide.getByRole("button", { name: "稍后再说" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(guide.getByRole("button", { name: "开始配置" })).toBeFocused();
  await page.screenshot({ path: info.outputPath("first-run-guide.png") });
  await guide.getByRole("button", { name: "稍后再说" }).click();
  await expect(guide).toHaveCount(0);
  await page.reload();
  await expect(page.getByRole("button", { name: "设置", exact: true })).toBeVisible();
  await expect(guide).toHaveCount(0);
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await page.getByRole("button", { name: "查看使用引导" }).click();
  await expect(guide).toBeVisible();
  await expect(page.getByRole("dialog")).toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(guide).toHaveCount(0);
});

test("guide opens existing configuration without sending a task", async ({ page }) => {
  await page.goto("/tests/ui/fixture.html?scenario=onboarding");
  await page.getByRole("button", { name: "开始配置" }).click();
  const settings = page.getByRole("dialog", { name: "连接你的模型" });
  await expect(settings).toBeVisible();
  await expect(page.getByRole("dialog")).toHaveCount(1);
  await expect(settings.getByLabel("模型名称", { exact: true })).toHaveValue("deepseek-v4-flash");
  await expect(settings.getByLabel("API Key", { exact: true })).toBeEmpty();
  // Local fixture only: no real endpoint or native credential store is used.
  await settings.getByLabel("API Key", { exact: true }).fill("fixture-not-a-real-key");
  await settings.getByRole("button", { name: "保存并使用" }).click();
  await expect(settings).toHaveCount(0);
  await expect(page.locator(".chat-message")).toHaveCount(0);
  await expect(page.getByRole("combobox", { name: "选择模型" })).toContainText("deepseek-v4-flash");
  await page.reload();
  await expect(page.getByRole("button", { name: "设置", exact: true })).toBeVisible();
  await expect(page.getByRole("dialog", { name: "欢迎使用 Simple" })).toHaveCount(0);
});

test("guide remains usable on a small dark window", async ({ page }, info) => {
  await page.setViewportSize({ width: 600, height: 500 });
  await page.emulateMedia({ colorScheme: "dark" });
  await page.goto("/tests/ui/fixture.html?scenario=onboarding");
  const guide = page.getByRole("dialog", { name: "欢迎使用 Simple" });
  await expect(guide).toBeVisible();
  expect(await guide.evaluate((element) => element.getBoundingClientRect().right <= innerWidth)).toBe(true);
  await guide.getByRole("button", { name: "稍后再说" }).scrollIntoViewIfNeeded();
  await page.screenshot({ path: info.outputPath("first-run-guide-dark.png") });
  await guide.getByRole("button", { name: "稍后再说" }).click();
  await expect(guide).toHaveCount(0);
});
