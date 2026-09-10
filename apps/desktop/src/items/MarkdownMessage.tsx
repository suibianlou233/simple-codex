import { Children, isValidElement, type ReactNode } from "react";
import ReactMarkdown, { defaultUrlTransform } from "react-markdown";
import { StoredImage } from "../components/StoredImage";
import remarkGfm from "remark-gfm";
import { CopyButton } from "../components/CopyButton";

export function codeText(children: ReactNode): string {
  return Children.toArray(children).map((child) => typeof child === "string" || typeof child === "number" ? String(child) : isValidElement<{ children?: ReactNode }>(child) ? codeText(child.props.children) : "").join("");
}

function CodeBlock({ children }: { children?: ReactNode }) {
  const code = Children.toArray(children).find((child) => isValidElement(child));
  const language = isValidElement<{ className?: string }>(code) ? code.props.className?.replace("language-", "") : undefined;
  return <div className="code-block"><header><span>{language ?? "代码"}</span><CopyButton text={codeText(children)} label="复制代码" /></header><pre>{children}</pre></div>;
}

export function MarkdownMessage({ content }: { content: string }) {
  return <div className="markdown-message"><ReactMarkdown remarkPlugins={[remarkGfm]} skipHtml urlTransform={url => /^simple-image:[a-f0-9]{64}$/.test(url) ? url : defaultUrlTransform(url)} components={{
    img: ({ src, alt }) => typeof src === "string" && /^simple-image:[a-f0-9]{64}$/.test(src) ? <StoredImage reference={src} name={alt ?? "图片"} /> : <span>{alt ?? "图片"}</span>,
    // Opening external destinations remains a bridge/product decision; never
    // turn model-generated paths into unrestricted browser navigation.
    a: ({ children, href }) => <span className="markdown-link" title={href}>{children}</span>,
    pre: ({ children }) => <CodeBlock>{children}</CodeBlock>,
  }}>{content}</ReactMarkdown></div>;
}
