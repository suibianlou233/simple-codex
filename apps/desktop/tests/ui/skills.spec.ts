import {test,expect} from "@playwright/test";
test("import, inspect inert content, disable and close the skill library",async({page})=>{
  await page.goto("/tests/ui/skills-fixture.html");
  await page.getByRole("button",{name:"导入",exact:true}).click();
  await expect(page.getByText("已启用",{exact:true})).toBeVisible();
  await page.getByRole("button",{name:"查看说明"}).click();
  await expect(page.locator("pre")).toContainText("<script>");
  expect(await page.evaluate(()=>"SKILL_EXECUTED" in window)).toBe(false);
  await page.getByRole("button",{name:"停用",exact:true}).click();
  await expect(page.getByText("已停用",{exact:true})).toBeVisible();
  await page.screenshot({path:"../../target/skills-manager.png"});
  await page.keyboard.press("Escape");
  await expect(page.getByText("已关闭",{exact:true})).toBeVisible();
});
