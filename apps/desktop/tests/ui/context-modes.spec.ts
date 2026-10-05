import {test,expect} from "@playwright/test";
test("modes isolate conversations and drafts and restore their own selection",async({page})=>{
 await page.goto("/tests/ui/fixture.html?scenario=contexts");const mode=page.getByRole("combobox",{name:"工作模式"}),input=page.getByRole("textbox",{name:"给 Simple 的任务"});
 await input.fill("Simple 未发送草稿");await mode.selectOption("code");await expect(input).toHaveValue("");await expect(page.getByRole("combobox",{name:"当前对话"}).locator("option")).toHaveCount(1);await expect(page.locator(".timeline")).not.toContainText("检查任务切换的体验");
 await input.fill("Code 独立草稿");await mode.selectOption("simple");await expect(input).toHaveValue("Simple 未发送草稿");await expect(page.locator(".timeline")).toContainText("检查任务切换的体验");await mode.selectOption("code");await expect(input).toHaveValue("Code 独立草稿");
 await page.getByRole("button",{name:"发送",exact:true}).click();await expect(page.locator(".timeline")).toContainText("Code 独立草稿");await expect(page.getByRole("combobox",{name:"当前对话"}).locator("option")).toHaveCount(2);
 await mode.selectOption("simple");await expect(input).toHaveValue("Simple 未发送草稿");await expect(page.locator(".timeline")).not.toContainText("Code 独立草稿");await mode.selectOption("code");await expect(page.locator(".timeline")).toContainText("Code 独立草稿");
 // Fixture resets its backend on reload; stored mode selection must not revive a missing/wrong-mode task.
 await page.reload();await expect(mode).toHaveValue("code");await expect(page.locator(".timeline")).not.toContainText("检查任务切换的体验");
});
