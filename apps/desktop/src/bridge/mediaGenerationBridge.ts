import { invoke } from "@tauri-apps/api/core";
import type { AttachmentSummary } from "./types";

export type MediaSettings = {
  baseUrl: string;
  imageModel: string;
  videoModel: string;
  hasCredential: boolean;
};

export type MediaJobStatus =
  | "submitting"
  | "queued"
  | "running"
  | "succeeded"
  | "failed"
  | "cancelled"
  | "expired"
  | "uncertain";

export type MediaJob = {
  id: string;
  projectId: string;
  kind: "image" | "video";
  model: string;
  status: MediaJobStatus;
  prompt: string;
  createdAtMs: number;
  updatedAtMs: number;
  providerTaskId?: string | null;
  assetRef?: string | null;
  assetPath?: string | null;
  mimeType?: string | null;
  error?: string | null;
  usageTokens?: number | null;
};

export type MediaAssetInfo = {
  reference: string;
  kind: "image" | "video";
  mimeType: string;
  width?: number | null;
  height?: number | null;
};

export type SaveMediaSettingsInput = {
  baseUrl: string;
  imageModel: string;
  videoModel: string;
  apiKey?: string;
};

export const mediaGenerationBridge = {
  settings: () => invoke<MediaSettings>("load_media_settings"),
  saveSettings: (input: SaveMediaSettingsInput) =>
    invoke<MediaSettings>("save_media_settings", { input }),
  jobs: (projectId: string) => invoke<MediaJob[]>("list_media_jobs", { projectId }),
  generateImage: (input: {
    projectId: string;
    prompt: string;
    model?: string;
    size: "1K" | "1.5K" | "2K";
    reference?: string;
    watermark: boolean;
  }) => invoke<MediaJob>("generate_image", { input }),
  createVideo: (input: {
    projectId: string;
    prompt: string;
    model?: string;
    resolution: "480p" | "720p" | "1080p";
    ratio: "adaptive" | "16:9" | "9:16" | "1:1" | "4:3" | "3:4" | "21:9";
    duration: number;
    firstFrame?: string;
    generateAudio: boolean;
    watermark: boolean;
  }) => invoke<MediaJob>("create_video", { input }),
  refreshVideo: (jobId: string) => invoke<MediaJob>("refresh_video", { jobId }),
  cancelVideo: (jobId: string) => invoke<MediaJob>("cancel_video", { jobId }),
  inspectAsset: (projectId: string, reference: string) =>
    invoke<MediaAssetInfo>("inspect_media_asset", { projectId, reference }),
  readImageDataUrl: (projectId: string, reference: string) =>
    invoke<string>("read_media_image_data_url", { projectId, reference }),
  readAsset: (projectId: string, reference: string) =>
    invoke<ArrayBuffer | Uint8Array | number[]>("read_media_asset", { projectId, reference }),
  asAttachment: (projectId: string, reference: string) =>
    invoke<AttachmentSummary>("media_asset_as_attachment", { projectId, reference }),
};
