import {test,expect} from "@playwright/test";
test("steer targets current turn; queued messages execute once in sequence",async({page})=>{await page.goto("/tests/ui/queue.html");const input=page.getByRole("textbox",{name:"给 Simple 的任务"});await input.fill("补充约束");await page.getByRole("button",{name:"立即补充"}).click();expect(await page.evaluate(()=>(window as unknown as {__steers:unknown[]}).__steers)).toEqual([{turnId:"first",content:"补充约束"}]);for(const text of ["next one","next two"]){await input.fill(text);await page.getByRole("button",{name:"加入队列"}).click();}await expect(page.getByLabel("已发送消息")).toHaveText("[]");await page.getByRole("button",{name:"完成当前轮"}).click();await expect(page.getByLabel("已发送消息")).toHaveText('["next one"]');await page.getByRole("button",{name:"完成当前轮"}).click();await expect(page.getByLabel("已发送消息")).toHaveText('["next one","next two"]');await page.getByRole("button",{name:"完成当前轮"}).click();await expect(page.getByLabel("已发送消息")).toHaveText('["next one","next two"]');});
test("failed rounds pause queue, uncertain sends survive reload without resend",async({page})=>{await page.goto("/tests/ui/queue.html");await page.getByRole("textbox",{name:"给 Simple 的任务"}).fill("do once");await page.getByRole("button",{name:"加入队列"}).click();await page.getByRole("button",{name:"失败当前轮"}).click();await expect(page.getByLabel("已发送消息")).toHaveText("[]");await page.getByLabel("模拟发送结果不明").check();await page.getByRole("button",{name:"现在发送下一条"}).click();await expect(page.getByRole("alert")).toContainText("不会自动重发");await page.reload();await page.getByRole("button",{name:"完成当前轮"}).click();await expect(page.getByLabel("已发送消息")).toHaveText("[]");await expect(page.getByRole("button",{name:"已核对，移除待确认项"})).toBeVisible();});


test("main composer queues Enter, keeps Shift Enter and IME input, and hides the old top panel",async({page})=>{
 await page.goto("/tests/ui/queue.html");const input=page.getByRole("textbox",{name:"给 Simple 的任务"});
 await expect(page.locator("textarea:visible")).toHaveCount(1);await expect(page.locator(".run-instructions")).toHaveCount(0);await expect(page.getByRole("region",{name:"待发送任务"})).toHaveCount(0);
 await input.fill("第一条");await input.press("Shift+Enter");await expect(input).toHaveValue("第一条\n");await input.press("Enter");await expect(input).toHaveValue("");
 await input.fill("输入法内容");await input.dispatchEvent("compositionstart");await input.dispatchEvent("keydown",{key:"Enter",code:"Enter",isComposing:true,keyCode:229});await expect(input).toHaveValue("输入法内容");await input.dispatchEvent("compositionend");
 await expect(page.locator(".composer-queue li")).toHaveCount(1);await input.press("Enter");await expect(page.locator(".composer-queue li")).toHaveCount(2);
 await page.getByRole("button",{name:"移除排队任务"}).last().click();await expect(page.locator(".composer-queue li")).toHaveCount(1);
 await page.getByRole("button",{name:"完成当前轮"}).click();await expect(page.getByLabel("已发送消息")).toContainText("第一条");
});

test("storage failure preserves the input and does not send",async({page})=>{
 await page.goto("/tests/ui/queue.html");await page.evaluate(()=>{const original=Storage.prototype.setItem;Storage.prototype.setItem=function(key,value){if(key.startsWith("simple-code-queue:"))throw new Error("quota");return original.call(this,key,value);};});
 const input=page.getByRole("textbox",{name:"给 Simple 的任务"});await input.fill("保留这条任务");await input.press("Enter");await expect(page.getByRole("alert")).toContainText("草稿已保留");await expect(input).toHaveValue("保留这条任务");await expect(page.getByLabel("已发送消息")).toHaveText("[]");
});


test("workbench puts queued tasks by the main input and preserves the new draft",async({page},info)=>{
 await page.goto("/tests/ui/fixture.html?scenario=running");
 const input=page.getByRole("textbox",{name:"给 Simple 的任务"});await expect(input).toBeEnabled();
 await expect(page.locator(".run-instructions")).toHaveCount(0);
 await input.fill("完成后整理 README");await input.press("Enter");await expect(input).toHaveValue("");
 await expect(page.locator(".composer-shell .composer-queue")).toContainText("完成后整理 README");
 await input.fill("然后检查测试结果");await input.press("Enter");await expect(input).toHaveValue("");
 await expect(page.locator(".composer-queue li")).toHaveCount(2);await expect(page.getByRole("button",{name:"停止回复",exact:true})).toBeEnabled();
 await page.screenshot({path:info.outputPath("composer-queue.png")});
});
