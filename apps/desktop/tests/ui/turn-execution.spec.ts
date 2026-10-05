import {test,expect} from "@playwright/test";
test("elapsed time stays above final answer and expands commentary on click",async({page})=>{
 await page.goto("/tests/ui/reasoning.html");await page.getByRole("button",{name:"完成任务",exact:true}).click();
 const details=page.locator(".turn-execution");await expect(details).toHaveCount(1);await expect(details.locator(":scope > summary")).toHaveText("用时 12秒›");await expect(details).not.toHaveAttribute("open","");
 await expect(page.getByText("已完成界面改造。执行说明支持折叠，最终回复保持独立显示。",{exact:true})).toBeVisible();
 await expect(page.locator(".process-reasoning")).toHaveCount(0);
 await details.locator(":scope > summary").click();await expect(details).toHaveAttribute("open","");await expect(details.locator(".chat-commentary").first()).toBeVisible();
 await details.locator(":scope > summary").press("Enter");await expect(details).not.toHaveAttribute("open","");
});
test("text without commentary still has a duration and steps are inside its disclosure",async({page},info)=>{
 await page.goto("/tests/ui/fixture.html?scenario=long");const details=page.locator(".turn-execution");await expect(details).toHaveCount(1);await expect(details.locator(":scope > summary")).toContainText("用时 0秒");
 await expect(details.locator(".execution-step-list")).toBeHidden();await details.locator(":scope > summary").click();await expect(details.locator(".execution-step-list")).toBeVisible();
 await details.locator(":scope > summary").click();await page.screenshot({path:info.outputPath("elapsed-disclosure.png")});
});

test("live steps expose commands and output on demand with a compact timer",async({page},info)=>{
 await page.goto("/tests/ui/fixture.html?scenario=running");
 const timer=page.locator(".execution-live-duration");
 await expect(timer).toHaveCSS("font-size","12px");
 await expect(timer.locator(".work-process-timer")).toHaveCSS("font-size","12px");
 const step=page.locator(".execution-step").first();
 await expect(step.locator("summary")).toContainText("正在执行");
 await expect(step.locator(".execution-step-body")).toHaveCount(0);
 await step.locator("summary").click();
 await expect(step.locator("pre").first()).toHaveText("Get-Content AGENTS.md");
 await expect(step.locator("pre").last()).toHaveText("模拟命令输出");
 await page.screenshot({path:info.outputPath("live-command.png")});
});
