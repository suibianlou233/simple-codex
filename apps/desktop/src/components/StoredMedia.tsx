import { useContext, useEffect, useState } from "react";
import { mediaGenerationBridge, type MediaAssetInfo } from "../bridge/mediaGenerationBridge";
import { AttachmentProjectContext } from "./StoredImage";

type StoredMediaBridge = Pick<
  typeof mediaGenerationBridge,
  "inspectAsset" | "readImageDataUrl" | "readAsset"
>;

type LoadedMedia = {
  key: string;
  asset?: MediaAssetInfo;
  url?: string;
  poster?: string;
};

type CachedMedia = Required<Pick<LoadedMedia, "asset" | "url">> & Pick<LoadedMedia, "poster">;
type MediaCacheEntry = {
  users: number;
  promise: Promise<CachedMedia>;
  value?: CachedMedia;
  cleanup?: ReturnType<typeof setTimeout>;
};

const mediaCache = new Map<string, MediaCacheEntry>();

function dataUrlObjectUrl(dataUrl: string, expectedMime: string): string {
  const match = /^data:([^;,]+);base64,([a-z0-9+/=]+)$/i.exec(dataUrl);
  if (!match || match[1] !== expectedMime) throw new Error("图片数据格式无效");
  const binary = atob(match[2]);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return URL.createObjectURL(new Blob([bytes], { type: expectedMime }));
}

function acquireMedia(
  key: string,
  projectId: string,
  reference: string,
  expectedKind: "image" | "video" | undefined,
  source: StoredMediaBridge,
): MediaCacheEntry {
  const existing = mediaCache.get(key);
  if (existing) {
    existing.users += 1;
    if (existing.cleanup) clearTimeout(existing.cleanup);
    existing.cleanup = undefined;
    return existing;
  }
  const entry: MediaCacheEntry = {
    users: 1,
    promise: source.inspectAsset(projectId, reference).then(async asset => {
      if (expectedKind && asset.kind !== expectedKind) throw new Error("媒体类型不匹配");
      let url: string;
      if (asset.kind === "image") {
        url = dataUrlObjectUrl(await source.readImageDataUrl(projectId, reference), asset.mimeType);
      } else {
        const payload = await source.readAsset(projectId, reference);
        const bytes = payload instanceof ArrayBuffer
          ? payload
          : Uint8Array.from(payload).buffer as ArrayBuffer;
        url = URL.createObjectURL(new Blob([bytes], { type: asset.mimeType }));
      }
      const value = { asset, url };
      entry.value = value;
      return value;
    }),
  };
  entry.promise.catch(() => {
    if (mediaCache.get(key) === entry) mediaCache.delete(key);
  });
  mediaCache.set(key, entry);
  return entry;
}

function releaseMedia(key: string, entry: MediaCacheEntry) {
  entry.users = Math.max(0, entry.users - 1);
  if (entry.users || entry.cleanup) return;
  // React/WebView can briefly unmount off-screen message content while scrolling.
  // Keep the immutable content-addressed asset warm long enough for a remount.
  entry.cleanup = setTimeout(() => {
    if (entry.users || mediaCache.get(key) !== entry) return;
    if (entry.value) URL.revokeObjectURL(entry.value.url);
    mediaCache.delete(key);
  }, 30_000);
}

function rememberVideoDimensions(
  key: string,
  width: number,
  height: number,
  update: (value: LoadedMedia) => void,
) {
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) return;
  const entry = mediaCache.get(key);
  if (!entry?.value || entry.value.asset.kind !== "video") return;
  if (entry.value.asset.width === width && entry.value.asset.height === height) return;
  entry.value = {
    ...entry.value,
    asset: { ...entry.value.asset, width, height },
  };
  update({ key, ...entry.value });
}

function rememberVideoPoster(
  key: string,
  video: HTMLVideoElement,
  update: (value: LoadedMedia) => void,
) {
  const entry = mediaCache.get(key);
  if (!entry?.value || entry.value.asset.kind !== "video" || entry.value.poster) return;
  const width = video.videoWidth;
  const height = video.videoHeight;
  if (!width || !height) return;
  try {
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext("2d");
    if (!context) return;
    context.drawImage(video, 0, 0, width, height);
    const poster = canvas.toDataURL("image/jpeg", 0.82);
    if (!poster.startsWith("data:image/jpeg;base64,")) return;
    entry.value = { ...entry.value, poster };
    update({ key, ...entry.value });
  } catch {
    // The playable video remains available even if this browser cannot capture a frame.
  }
}

export function StoredMedia({ reference, name, expectedKind, bridge }: {
  reference: string;
  name: string;
  expectedKind?: "image" | "video";
  bridge?: StoredMediaBridge;
}) {
  const projectId = useContext(AttachmentProjectContext);
  const key = `${projectId}:${reference}`;
  const [media, setMedia] = useState<LoadedMedia | undefined>(() => {
    const cached = mediaCache.get(key)?.value;
    return cached ? { key, ...cached } : undefined;
  });
  useEffect(() => {
    let cancelled = false;
    if (!projectId || !/^simple-media:[a-f0-9]{64}$/.test(reference)) return;
    const source = bridge ?? mediaGenerationBridge;
    const entry = acquireMedia(key, projectId, reference, expectedKind, source);
    if (entry.value) setMedia({ key, ...entry.value });
    void entry.promise.then(value => {
      if (!cancelled) setMedia({ key, ...(entry.value ?? value) });
    }).catch(() => {
      if (!cancelled) setMedia({ key });
    });
    return () => {
      cancelled = true;
      releaseMedia(key, entry);
    };
  }, [bridge, expectedKind, key, projectId, reference]);

  if (media?.key !== key || !media.asset || !media.url) {
    return <span className="image-placeholder">{media?.key === key ? "媒体不可用" : "正在加载媒体"}：{name}</span>;
  }
  return media.asset.kind === "video"
    ? <span
        className="stored-media stored-video-frame"
        style={{ aspectRatio: media.asset.width && media.asset.height ? `${media.asset.width} / ${media.asset.height}` : "16 / 9" }}
      >
        {media.poster ? <img className="stored-video-poster" src={media.poster} alt="" aria-hidden="true" /> : null}
        <video
          className="stored-video"
          src={media.url}
          poster={media.poster}
          controls
          preload="auto"
          playsInline
          width={media.asset.width ?? undefined}
          height={media.asset.height ?? undefined}
          aria-label={name || "生成视频"}
          onLoadedMetadata={event => rememberVideoDimensions(
            key,
            event.currentTarget.videoWidth,
            event.currentTarget.videoHeight,
            value => setMedia(value),
          )}
          onLoadedData={event => rememberVideoPoster(key, event.currentTarget, value => setMedia(value))}
        />
      </span>
    : <img
        className="stored-media stored-generated-image"
        src={media.url}
        alt={name || "生成图片"}
        width={media.asset.width ?? undefined}
        height={media.asset.height ?? undefined}
        loading="eager"
      />;
}
