export type TerminalFileLink={start:number;end:number;text:string;target:string};
// Quoted paths preserve spaces; unquoted paths support Unicode and Windows drive letters.
export function terminalFileLinks(text:string):TerminalFileLink[]{
 const result:TerminalFileLink[]=[];
 const patterns=[/"([^"\r\n]+\.[a-zA-Z0-9]+)"(?:, line (\d+)|:(\d+)(?::(\d+))?)/g,/(?:[A-Za-z]:[\\/])?(?:[^\s"'<>|():]+[\\/])*[^\s"'<>|():]+\.[a-zA-Z0-9]+(?::\d+(?::\d+)?|\(\d+,\d+\))/g];
 for(const [index,pattern] of patterns.entries())for(const match of text.matchAll(pattern)){const start=match.index!,end=start+match[0].length;if(result.some(r=>start<r.end&&end>r.start))continue;result.push({start,end,text:match[0],target:index===0?`${match[1]}:${match[2]??match[3]}${match[4]?":"+match[4]:""}`:match[0].replace(/\((\d+),(\d+)\)$/,":$1:$2")});}
 return result.sort((a,b)=>a.start-b.start);
}
