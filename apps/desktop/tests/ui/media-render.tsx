import { createRoot } from "react-dom/client";
import { useState } from "react";
import { AttachmentProjectContext } from "../../src/components/StoredImage";
import { StoredMedia } from "../../src/components/StoredMedia";

const reference = `simple-media:${"b".repeat(64)}`;
const videoReference = `simple-media:${"c".repeat(64)}`;
// A valid 1 x 1 PNG. The production backend returns the same validated data-URL shape.
const injected = (window as Window & { __SIMPLE_MEDIA_DATA_URL__?: string }).__SIMPLE_MEDIA_DATA_URL__;
const injectedVideo = (window as Window & { __SIMPLE_MEDIA_VIDEO__?: string }).__SIMPLE_MEDIA_VIDEO__;
const png = injected ?? "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
const bridge = {
  inspectAsset: async (_projectId: string, requested: string) => requested === videoReference
    ? ({ reference: videoReference, kind: "video" as const, mimeType: "video/mp4" })
    : ({ reference, kind: "image" as const, mimeType: "image/png", width: 1152, height: 864 }),
  readImageDataUrl: async () => {
    const state = window as Window & { __SIMPLE_MEDIA_READS__?: number };
    state.__SIMPLE_MEDIA_READS__ = (state.__SIMPLE_MEDIA_READS__ ?? 0) + 1;
    return png;
  },
  readAsset: async () => {
    const state = window as Window & { __SIMPLE_VIDEO_READS__?: number };
    state.__SIMPLE_VIDEO_READS__ = (state.__SIMPLE_VIDEO_READS__ ?? 0) + 1;
    const binary = atob(injectedVideo ?? "");
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
    return bytes.buffer;
  },
};

function Fixture() {
  const [shown, setShown] = useState(true);
  return <>
    <button type="button" onClick={() => setShown(value => !value)}>{shown ? "隐藏" : "显示"}</button>
    <div data-testid="media-scroll" style={{ height: 320, overflow: "auto" }}>
      <div style={{ height: 420 }}>图片上方内容</div>
      <AttachmentProjectContext.Provider value="E:\\fixture-project">
        {shown ? <StoredMedia reference={reference} name="生成图片" expectedKind="image" bridge={bridge} /> : null}
        {shown && injectedVideo ? <StoredMedia reference={videoReference} name="生成视频" expectedKind="video" bridge={bridge} /> : null}
      </AttachmentProjectContext.Provider>
      <div data-testid="after-image" style={{ height: 560 }}>图片下方文字</div>
    </div>
  </>;
}

createRoot(document.getElementById("root")!).render(<Fixture />);
