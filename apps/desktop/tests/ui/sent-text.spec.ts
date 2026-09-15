import { test,expect } from "@playwright/test";
test("sent body remains a card, survives reload and opens intact in a resizable right panel",async({page})=>{
  await page.goto("/tests/ui/frontend-fixes.html?scenario=sent-text");
  const body="第一章 陨落的天才\n"+"这是需要保留的正文。😀\n".repeat(3000);
  await expect(page.locator(".chat-user .message-text")).toHaveText("请提取前三章并进行分析");
  await expect(page.locator(".chat-user")).not.toContainText("这是需要保留的正文");
  const bubbleColor=await page.locator(".chat-user .message-text").evaluate(el=>getComputedStyle(el).backgroundColor);
  await page.getByRole("button",{name:/打开正文 1/}).click();
  await expect(page.getByRole("region",{name:"正文阅读栏"})).toBeVisible();
  await expect(page.getByLabel("正文全文")).toHaveText(body);
  await expect(page.getByRole("separator",{name:"调整右侧栏宽度"})).toBeVisible();
  await page.screenshot({path:"../../target/sent-text-reader.png"});
  await page.getByRole("button",{name:"关闭正文"}).click();
  await expect(page.getByRole("region",{name:"正文阅读栏"})).toHaveCount(0);
  // This route skips metadata registration, exercising durable offsets after reload.
  await page.goto("/tests/ui/frontend-fixes.html?scenario=sent-text-reloaded");
  await expect(page.locator(".chat-user .message-text")).toHaveText("请提取前三章并进行分析");
  await page.getByRole("button",{name:/打开正文 1/}).click();
  await expect(page.getByLabel("正文全文")).toHaveText(body);
  await page.getByRole("button",{name:/新任务/}).click();
  await expect(page.getByRole("region",{name:"正文阅读栏"})).toHaveCount(0);
  await page.goto("/tests/ui/frontend-fixes.html?scenario=completed");
  expect(await page.locator(".chat-user .message-text").evaluate(el=>getComputedStyle(el).backgroundColor)).toBe(bubbleColor);
});
test("old long messages collapse without stored presentation metadata",async({page})=>{
  await page.goto("/tests/ui/frontend-fixes.html?scenario=sent-text-old");
  await expect(page.locator(".chat-user .message-text")).toHaveCount(0);
  await page.getByRole("button",{name:/打开正文 1/}).click();
  await expect(page.getByLabel("正文全文")).toContainText("请提取前三章并进行分析");
});
