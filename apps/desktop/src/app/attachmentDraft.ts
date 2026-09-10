import type { AttachmentSummary } from "../bridge/types";

export const isImageAttachment = (item: AttachmentSummary) => item.path.startsWith("simple-image:");

// All entry points share one atomic draft update; never truncate silently.
export function normalizeAttachments(items: AttachmentSummary[]): AttachmentSummary[] {
  const unique = [...new Map(items.map(item => [item.path, item])).values()];
  if (unique.filter(isImageAttachment).length > 8) throw new Error("每条消息最多 8 张图片，请先移除一些图片");
  return unique;
}

export function validateImageFiles(files: File[]): void {
  if (files.length > 8) throw new Error("每次最多添加 8 张图片");
  for (const file of files) {
    if (!/\.(png|jpe?g|gif|webp)$/i.test(file.name) && !/^image\/(png|jpeg|gif|webp)$/i.test(file.type)) throw new Error("请选择 PNG、JPG、GIF 或 WebP 图片");
    if (file.size > 4 * 1024 * 1024) throw new Error("每张图片最多 4 MiB");
  }
}

export function buildComposerMessage(content: string, attachments: AttachmentSummary[]): string {
  const selected = normalizeAttachments(attachments);
  const files = selected.filter(item => !isImageAttachment(item));
  const images = selected.filter(isImageAttachment);
  const block = [
    files.length ? `附件（项目内文本文件，请使用 read_file 读取）：\n${files.map(item => `- ${item.path}`).join("\n")}` : "",
    ...images.map(item => `![${item.name.replace(/[\[\]\\\r\n]/g, "")}](${item.path})`),
  ].filter(Boolean).join("\n\n");
  // Independent attachment rows precede draft text, including unfinished fences.
  return [block, content.trim() || "请阅读并分析这些附件。"].filter(Boolean).join("\n\n");
}
