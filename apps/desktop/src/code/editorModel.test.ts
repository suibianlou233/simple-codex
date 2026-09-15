import { describe, expect, it } from "vitest";
import { reconcile, projectRelativePath, type Buffer } from "./editorModel";
describe("editor external changes",()=>{
  const file:Buffer={projectId:"project",path:"a.ts",content:"old",saved:"old",sha256:"old-hash"};
  const disk={path:"a.ts",content:"AI edit",sha256:"new-hash"};
  it("reloads a clean buffer but retains a dirty buffer and its save precondition",()=>{
    expect(reconcile(file,disk)).toMatchObject({content:"AI edit",saved:"AI edit",sha256:"new-hash"});
    expect(reconcile({...file,content:"manual edit"},disk)).toMatchObject({content:"manual edit",saved:"old",sha256:"old-hash",disk});
  });
  it("opens only references inside the project",()=>{
    expect(projectRelativePath("C:/work/app/src/a.ts:12","C:/work/app")).toBe("src/a.ts");
    expect(projectRelativePath("src/a.ts#L12","C:/work/app")).toBe("src/a.ts");
    for(const href of ["https://example.com/a.ts","../secret","C:/elsewhere/a.ts","%2e%2e/secret","//server/file"])
      expect(projectRelativePath(href,"C:/work/app")).toBeUndefined();
  });
});
