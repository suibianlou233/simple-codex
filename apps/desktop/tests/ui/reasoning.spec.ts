import { test, expect } from "@playwright/test";

test.beforeEach(async ({page}) => {
  await page.route("**/*", route => new URL(route.request().url()).hostname === "127.0.0.1" ? route.continue() : route.abort());
  await page.goto("/tests/ui/reasoning.html");
});
const trigger = '.process-reasoning [data-slot="reasoning-trigger"]';
const text = '.process-reasoning [data-slot="reasoning-text"]';

test("stream opens; final response collapses it; keyboard can reopen it", async ({page}) => {
  await expect(page.locator(trigger)).toHaveAttribute("aria-expanded", "true");
  await page.getByRole("button",{name:"开始最终回复",exact:true}).click();
  await expect(page.locator(trigger)).toHaveAttribute("aria-expanded", "false");
  await expect(page.getByText("已完成界面改造。执行说明支持折叠，最终回复保持独立显示。",{exact:true})).toBeVisible();
  await page.locator(trigger).focus(); await page.keyboard.press("Enter");
  await expect(page.locator(text)).toBeVisible();
  await page.getByRole("button",{name:"完成任务",exact:true}).click();
  const completed = page.locator(".turn-execution");
  await expect(page.locator(trigger)).toHaveCount(0);
  await expect(completed).not.toHaveAttribute("open", "");
  await expect(completed.locator(":scope > summary")).toContainText("用时 12秒");
  await completed.locator(":scope > summary").focus();
  await page.keyboard.press("Enter");
  await expect(completed.locator(".chat-commentary")).toBeVisible();
  await expect(page.getByText("已完成界面改造。执行说明支持折叠，最终回复保持独立显示。",{exact:true})).toBeVisible();
});

test("manual collapse survives deltas; another task starts with a fresh disclosure", async ({page}) => {
  await page.locator(trigger).click();
  await page.getByRole("button",{name:"追加内容",exact:true}).click();
  await expect(page.locator(trigger)).toHaveAttribute("aria-expanded", "false");
  await page.getByRole("button",{name:"切换任务",exact:true}).click();
  await expect(page.locator(trigger)).toHaveAttribute("aria-expanded", "true");
  await page.getByRole("button",{name:"停止任务",exact:true}).click();
  await expect(page.locator(trigger)).toHaveCount(0);
  await expect(page.locator(".turn-execution")).not.toHaveAttribute("open", "");
  await expect(page.getByText("任务已停止",{exact:true})).toBeVisible();
});

test("follows streaming content and pauses when the reader scrolls up", async ({page}) => {
  const box=page.locator(text);
  await page.getByRole("button",{name:"追加内容",exact:true}).click();
  await expect.poll(()=>box.evaluate(el=>Math.abs(el.scrollHeight-el.clientHeight-el.scrollTop))).toBeLessThan(2);
  await box.hover(); await page.mouse.wheel(0,-250);
  await expect.poll(()=>box.evaluate(el=>el.scrollHeight-el.clientHeight-el.scrollTop)).toBeGreaterThan(50);
  const top=await box.evaluate(el=>el.scrollTop);
  await page.getByRole("button",{name:"追加内容",exact:true}).click();
  await expect.poll(()=>box.evaluate(el=>el.scrollTop)).toBe(top);
  await box.evaluate(el=>{el.scrollTop=el.scrollHeight;});
  await expect.poll(()=>box.evaluate(el=>Math.abs(el.scrollHeight-el.clientHeight-el.scrollTop))).toBeLessThan(2);
  await page.getByRole("button",{name:"追加内容",exact:true}).click();
  await expect.poll(()=>box.evaluate(el=>Math.abs(el.scrollHeight-el.clientHeight-el.scrollTop))).toBeLessThan(2);
});

test("light and dark previews fit a narrow window and respect reduced motion", async ({page}, info) => {
  await page.screenshot({path:info.outputPath("reasoning-light.png"), fullPage:true});
  await page.getByRole("button",{name:"切换主题",exact:true}).click();
  await page.getByRole("button",{name:"追加内容",exact:true}).click();
  await page.screenshot({path:info.outputPath("reasoning-dark.png"), fullPage:true});
  await page.setViewportSize({width:390,height:844});
  await page.emulateMedia({reducedMotion:"reduce"});
  await expect.poll(()=>page.evaluate(()=>document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await expect(page.locator('[data-slot="reasoning-trigger-label"]')).toHaveCSS("animation-name","none");
  await page.screenshot({path:info.outputPath("reasoning-narrow.png"), fullPage:true});
});
