import { Fragment, useEffect, useState } from "react";
import { Icon } from "../components/Icon";
import { fallbackTextRanges, sentTextRanges } from "../app/sentText";

export type TextDocument = { title: string; text: string };
export function UserMessage({text,onOpen}: {text:string;onOpen:(document:TextDocument)=>void}) {
  const [loaded, setLoaded] = useState<{text:string;ranges:Awaited<ReturnType<typeof sentTextRanges>>}>();
  useEffect(() => {
    let current = true;
    void sentTextRanges(text).then(ranges => {if(current) setLoaded({text,ranges});});
    return () => {current=false;};
  }, [text]);
  const ranges = loaded?.text === text ? loaded.ranges : fallbackTextRanges(text);
  if (!ranges.length) return <p className="message-text">{text}</p>;
  return <div className="sent-text-content">
    {ranges.map((range,index) => {
      const before = text.slice(index ? ranges[index-1].end : 0,range.start);
      const body = text.slice(range.start,range.end);
      const title = body.trim().split(/\r?\n/,1)[0].slice(0,80) || "正文";
      return <Fragment key={range.start}>
        {before.trim() ? <p className="message-text">{before}</p> : null}
        <button type="button" className="sent-text-card" aria-label={`打开正文 ${index+1}：${title}`} onClick={()=>onOpen({title,text:body})}>
          <Icon name="text" size={16}/><span>{title}</span><small>{Array.from(body).length.toLocaleString()} 字</small>
        </button>
      </Fragment>;
    })}
    {text.slice(ranges.at(-1)!.end).trim() ? <p className="message-text">{text.slice(ranges.at(-1)!.end)}</p> : null}
  </div>;
}
