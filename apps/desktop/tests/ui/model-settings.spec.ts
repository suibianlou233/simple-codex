import {test,expect} from "@playwright/test";
test("Token Plan preset uses its own key and creates a profile without overwriting DeepSeek",async({page})=>{
  await page.goto("/tests/ui/model-settings.html");
  await expect(page.getByLabel("无响应等待时间（秒）")).toHaveValue("240");
  await page.getByRole("button",{name:"保存并使用"}).click();
  expect(JSON.parse(await page.getByTestId("saved").textContent() ?? "{}").timeoutMs).toBe(240000);
  await page.getByRole("button",{name:"千问 Token Plan",exact:true}).click();
  await expect(page.getByLabel("接口地址")).toHaveValue("https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1");
  await expect(page.getByLabel("模型名称")).toHaveValue("qwen3.8-max");
  await expect(page.getByLabel("API Key")).toHaveAttribute("required","");
  await page.getByLabel("API Key").fill("sk-sp-fixture-not-a-real-key");
  await page.getByLabel("无响应等待时间（秒）").fill("600");
  await page.getByRole("button",{name:"保存并使用"}).click();
  const saved=JSON.parse(await page.getByTestId("saved").textContent() ?? "{}");
  expect(saved).toMatchObject({dialect:"qwen",model:"qwen3.8-max",timeoutMs:600000,contextWindowTokens:1000000});
  expect(saved.profileId).toBeUndefined();
});
