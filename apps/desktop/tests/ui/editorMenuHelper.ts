import type {Page} from "@playwright/test";
export async function editorMenu(page:Page,label:string){await page.getByRole("button",{name:"编辑器操作",exact:true}).click();return page.getByRole("menuitem").filter({has:page.locator("span",{hasText:new RegExp(`^${label}$`)})});}
