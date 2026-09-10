if (location.href !== a.expected) return {error: "网页已改变，请重新读取页面"};
const visible = e => !!(e.getClientRects().length && getComputedStyle(e).visibility !== "hidden");
if (a.action === "read") {
  const selector = e => {
    if (e.id && document.querySelectorAll("#" + CSS.escape(e.id)).length === 1) return "#" + CSS.escape(e.id);
    const parts = [];
    for (let node = e; node && node !== document.documentElement; node = node.parentElement) {
      const tag = node.localName;
      const siblings = Array.from(node.parentElement?.children || []).filter(n => n.localName === tag);
      parts.unshift(tag + ":nth-of-type(" + (siblings.indexOf(node) + 1) + ")");
    }
    return "html > " + parts.join(" > ");
  };
  const elements = Array.from(document.querySelectorAll("a,button,input,textarea,select,[role=button]"))
    .filter(visible).slice(0, 100).map(e => ({
      selector: selector(e), tag: e.localName, type: e.getAttribute("type"),
      label: (e.getAttribute("aria-label") || e.innerText || e.getAttribute("placeholder") || "").slice(0, 160),
    }));
  return {title: document.title, url: location.href, text: (document.body?.innerText || "").slice(0, 24000), elements};
}
if (a.action === "back") { history.back(); return {requested: "back"}; }
if (a.action === "forward") { history.forward(); return {requested: "forward"}; }
if (a.action === "reload") { location.reload(); return {requested: "reload"}; }
if (a.action === "scroll") { window.scrollBy(0, a.delta ?? 600); return {x: scrollX, y: scrollY}; }
const matches = document.querySelectorAll(a.selector);
if (matches.length !== 1 || !visible(matches[0])) return {error: "需要唯一且可见的元素，请重新读取页面"};
const element = matches[0];
if (element instanceof HTMLInputElement && ["password", "file", "hidden"].includes(element.type)) return {error: "请由用户手动操作密码或文件输入框"};
if (element.disabled || element.getAttribute("aria-disabled") === "true") return {error: "该元素不可操作"};
if (a.action === "click") { element.scrollIntoView({block: "center"}); element.click(); return {clicked: true}; }
if (a.action === "fill") {
  if (!(element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement)) return {error: "只支持可见的普通文本输入框"};
  if (element.readOnly) return {error: "该输入框为只读"};
  const prototype = element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(prototype, "value")?.set;
  if (!setter) return {error: "该输入框不支持填写"};
  element.focus(); setter.call(element, a.text);
  element.dispatchEvent(new Event("input", {bubbles: true}));
  element.dispatchEvent(new Event("change", {bubbles: true}));
  return {filled: true};
}
return {error: "不支持的操作"};
