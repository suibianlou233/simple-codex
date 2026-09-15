import { test, expect, type Locator } from "@playwright/test";

test("failed submission retains the full draft and successful retry clears it",async ({page})=>{
  await page.goto("/tests/ui/frontend-fixes.html?scenario=paste-send");
  const input=page.getByRole("textbox",{name:"给 Simple 的任务"});
  await paste(input,"完整原文".repeat(1000),0,0);
  await expect(input).toBeFocused();
  await page.keyboard.insertText("补充要求");
  await page.getByRole("button",{name:"发送",exact:true}).click();
  await expect(page.getByRole("alert")).toContainText("这次操作未能完成");
  await expect(input).toHaveValue("补充要求");
  await page.getByRole("button",{name:"查看粘贴文本 1"}).click();
  await expect(page.getByRole("textbox",{name:"编辑粘贴文本"})).toHaveValue("完整原文".repeat(1000));
  await page.getByRole("button",{name:"取消",exact:true}).click();
  await page.getByRole("button",{name:"发送",exact:true}).click();
  await expect(input).toHaveValue("");
  await expect(page.locator(".pasted-text-chip")).toHaveCount(0);
});

async function paste(input: Locator, text: string, start?: number, end?: number) {
  await input.evaluate((element, data) => {
    const target = element as HTMLTextAreaElement;
    if (data.start !== undefined) target.setSelectionRange(data.start,data.end ?? data.start);
    const clipboardData = new DataTransfer(); clipboardData.setData("text/plain",data.text);
    element.dispatchEvent(new ClipboardEvent("paste",{clipboardData,bubbles:true,cancelable:true}));
  }, {text,start,end});
}

test("long pastes keep focus and allow typing, multiple blocks, editing and exact submission", async ({page}) => {
  await page.goto("/tests/ui/long-text.html");
  const input = page.getByRole("textbox",{name:"给 Simple 的任务"});
  const first = "中😀\n".repeat(6000);
  const second = "第二段".repeat(600);
  await input.fill("前文 后文");
  await paste(input, first, 3, 3);
  await expect(input).toBeVisible();
  await expect(input).toBeFocused();
  await expect(input).toHaveValue("后文");
  await input.press("Control+End");
  await page.keyboard.insertText(" 补充指令");
  await paste(input, second);
  await expect(input).toBeFocused();
  await page.keyboard.insertText(" 最后要求");
  await expect(input).toHaveValue(" 最后要求");
  await expect(page.locator(".pasted-text-chip")).toHaveCount(2);
  await page.getByRole("button",{name:"查看粘贴文本 1"}).click();
  const editor=page.getByRole("textbox",{name:"编辑粘贴文本"});
  await expect(editor).toHaveValue("前文 " + first);
  await editor.focus(); await editor.press("Control+End"); await page.keyboard.insertText("修订");
  await page.getByRole("button",{name:"保存",exact:true}).click();
  await expect(input).toBeFocused();
  await page.getByRole("button",{name:"发送",exact:true}).click();
  expect(await page.getByTestId("sent").textContent()).toBe("前文 " + first + "修订后文 补充指令" + second + " 最后要求");
  await page.getByRole("button",{name:"移除粘贴文本 1"}).click();
  await expect(input).toBeFocused();
  await expect(input).toHaveValue(" 最后要求");
  await page.getByRole("button",{name:"发送",exact:true}).click();
  expect(await page.getByTestId("sent").textContent()).toBe("后文 补充指令" + second + " 最后要求");
  await page.screenshot({path:"../../target/paste-cards.png"});
});

test("short paste is editable inline; canceling a block edit preserves its text",async ({page})=>{
  await page.goto("/tests/ui/long-text.html");
  const input=page.getByRole("textbox",{name:"给 Simple 的任务"});
  await input.fill("A OLD Z"); await paste(input,"短文",2,5);
  await expect(input).toHaveValue("A 短文 Z");
  await expect(page.locator(".pasted-text-chip")).toHaveCount(0);
  await paste(input,"x".repeat(1001),0,0);
  await page.getByRole("button",{name:"查看粘贴文本 1"}).click();
  await page.getByRole("textbox",{name:"编辑粘贴文本"}).fill("not saved");
  await page.getByRole("button",{name:"取消",exact:true}).click();
  await page.getByRole("button",{name:"发送",exact:true}).click();
  expect(await page.getByTestId("sent").textContent()).toBe("x".repeat(1001)+"A 短文 Z");
});

test("real workbench keeps paste blocks with their conversation drafts",async ({page})=>{
  await page.goto("/tests/ui/frontend-fixes.html?scenario=completed");
  const input=page.getByRole("textbox",{name:"给 Simple 的任务"});
  await paste(input,"existing task".repeat(200),0,0);
  await expect(input).toBeFocused();
  await page.keyboard.insertText("keep this instruction");
  await page.getByRole("button",{name:/新任务/}).click();
  await expect(page.locator(".pasted-text-chip")).toHaveCount(0);
  await expect(input).toHaveValue("");
  await paste(input,"new task".repeat(200),0,0);
  await page.locator(".task-row").first().click();
  await expect(input).toHaveValue("keep this instruction");
  await page.getByRole("button",{name:"查看粘贴文本 1"}).click();
  await expect(page.getByRole("textbox",{name:"编辑粘贴文本"})).toHaveValue("existing task".repeat(200));
});
