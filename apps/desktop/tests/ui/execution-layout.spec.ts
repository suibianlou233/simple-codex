import { test, expect } from "@playwright/test";
test("execution history stays compact and expands into ordered groups",async({page},info)=>{
  await page.goto("/tests/ui/fixture.html?scenario=execution-layout");
  await expect(page.locator(".work-process-live")).toContainText("正在处理任务");
  const history=page.locator(".execution-step-list");
  await expect(history).toBeVisible();
  await expect(history.locator(".execution-step")).toHaveCount(16);
  await expect(history.locator(".execution-step small",{hasText:"未成功"})).toHaveCount(2);
  await expect(history.locator(".execution-step small",{hasText:"正在执行"})).toHaveCount(1);
  await expect(page.locator("body")).not.toContainText("PRIVATE_");
  await page.screenshot({path:info.outputPath("execution-compact.png")});
  await history.locator(".execution-step").nth(7).locator("summary").click();
  await expect(history.locator(".execution-step").nth(7).locator(".execution-step-body")).toBeVisible();
  await page.screenshot({path:info.outputPath("execution-expanded.png")});
  await history.locator(".execution-step").nth(7).locator("summary").click();
  await expect(history.locator(".execution-step").nth(7)).not.toHaveAttribute("open","");
});
