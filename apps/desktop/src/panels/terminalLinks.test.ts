import {it,expect} from "vitest";
import {terminalFileLinks} from "./terminalLinks";
it("links Unicode Windows paths, quoted spaces, Python and TS diagnostics",()=>{
 expect(terminalFileLinks('错误 D:\\项目\\src\\main.ts:12:3')[0].target).toBe('D:\\项目\\src\\main.ts:12:3');
 expect(terminalFileLinks('File "D:/my project/main.py", line 42')[0].target).toBe('D:/my project/main.py:42');
 expect(terminalFileLinks('src/main.ts(2,4): error')[0].target).toBe('src/main.ts:2:4');
 expect(terminalFileLinks('"src/my main.ts":7:9')).toHaveLength(1);
});
