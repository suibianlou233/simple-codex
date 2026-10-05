import {editorMenu} from "./editorMenuHelper";
import {test,expect,type Page} from "@playwright/test";

async function openEditor(page:Page) {
  await page.goto("/tests/ui/fixture.html?scenario=code");
  await page.getByRole("combobox",{name:"工作模式"}).selectOption("code");
  await page.locator(".code-tree-row").getByRole("button",{name:"▸ ▣ src",exact:true}).click();
  await page.locator(".code-tree-row").getByRole("button",{name:"· main.ts",exact:true}).click();
  const editor=page.locator(".code-editor-area .monaco-editor .view-lines");
  await editor.click();await page.keyboard.press("Control+a");
  return editor;
}
test("inline edit previews, accepts in memory, then explicitly saves",async({page},info)=>{
  const editor=await openEditor(page);
  await page.keyboard.press("Control+k");
  await expect(page.getByRole("region",{name:"AI 选区修改"})).toBeVisible();
  await expect(page.getByRole("dialog",{name:"搜索任务"})).toHaveCount(0);
  await page.getByRole("textbox",{name:"选区修改要求"}).fill("更新欢迎语");
  await page.getByRole("button",{name:"生成建议",exact:true}).click();
  await expect(page.getByRole("region",{name:"修改差异"})).toContainText("Hello Cursor");
  await expect(editor).toContainText("Hello Simple");
  await expect.poll(()=>page.evaluate(()=>((window as unknown as {__editorSaves:unknown[]}).__editorSaves).length)).toBe(0);
  await page.screenshot({path:info.outputPath("inline-edit-preview.png")});
  await page.getByRole("button",{name:"接受修改",exact:true}).click();
  await expect(editor).toContainText("Hello Cursor");
  await expect(page.locator(".code-status")).toContainText("未保存");
  await (await editorMenu(page,"保存")).click();
  await expect(page.locator(".code-status")).not.toContainText("未保存");
});
test("manual edits invalidate generated proposals and cancel drops late responses",async({page})=>{
  const editor=await openEditor(page);
  await (await editorMenu(page,"AI 修改选区")).click();
  await page.getByRole("textbox",{name:"选区修改要求"}).fill("更新欢迎语");
  await page.getByRole("button",{name:"生成建议",exact:true}).click();
  await page.getByRole("button",{name:"取消生成",exact:true}).click();
  await page.waitForTimeout(300);
  await expect(page.getByRole("button",{name:"接受修改",exact:true})).toHaveCount(0);
  await page.getByRole("button",{name:"生成建议",exact:true}).click();
  await expect(page.getByRole("button",{name:"接受修改",exact:true})).toBeVisible();
  await editor.click();await page.keyboard.press("Control+End");await page.keyboard.type("// user edit");
  await expect(page.getByRole("button",{name:"接受修改",exact:true})).toBeDisabled();
  await expect(editor).toContainText("user edit");
});
test("disk updates are checked again at acceptance",async({page})=>{
  const editor=await openEditor(page);
  await (await editorMenu(page,"AI 修改选区")).click();
  await page.getByRole("textbox",{name:"选区修改要求"}).fill("更新欢迎语");
  await page.getByRole("button",{name:"生成建议",exact:true}).click();
  await expect(page.getByRole("button",{name:"接受修改",exact:true})).toBeVisible();
  await page.evaluate(async()=>{const path="/node_modules/@tauri-apps/api/core.js";await (await import(path)).invoke("test_external");});
  await page.getByRole("button",{name:"接受修改",exact:true}).click();
  await expect(page.getByRole("region",{name:"AI 选区修改"})).toContainText("文件已");
  await expect(editor).not.toContainText("Hello Cursor");
});
test("file references use unsaved buffers and show their source; review distinguishes task and Git",async({page},info)=>{
  const editor=await openEditor(page);
  await page.keyboard.press("Control+End");await page.keyboard.type("// draft only");
  await page.getByRole("textbox",{name:"给 Simple 的任务"}).press("@");
  const picker=page.getByRole("dialog",{name:"引用项目文件"});
  await expect(picker).toBeVisible();
  await picker.getByRole("textbox",{name:"搜索引用文件"}).fill("main");
  await picker.getByRole("button",{name:"src/main.ts",exact:true}).click();
  await expect(page.locator(".composer")).toContainText("src/main.ts · 全文");
  await expect(page.locator(".composer")).toContainText("未保存内容快照");
  await page.getByRole("button",{name:"查看粘贴文本 1",exact:true}).click();
  await expect(page.getByRole("dialog",{name:"粘贴文本"}).getByRole("textbox")).toHaveValue(/draft only/);
  await page.getByRole("dialog",{name:"粘贴文本"}).getByRole("button",{name:"取消",exact:true}).click();
  await (await editorMenu(page,"任务修改")).click();
  await expect(page.getByRole("region",{name:"任务修改审查"})).toContainText("new code");
  await page.screenshot({path:info.outputPath("task-diff-review.png")});
  await page.getByRole("button",{name:"Git 工作区",exact:true}).click();
  await expect(page.getByRole("region",{name:"任务修改审查"})).toContainText("包含你手动修改");
  await page.getByRole("button",{name:"返回编辑",exact:true}).click();
  await expect(editor).toContainText("draft only");
});
