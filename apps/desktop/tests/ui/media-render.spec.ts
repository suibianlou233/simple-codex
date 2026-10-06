import { expect, test } from "@playwright/test";
import { readFile } from "node:fs/promises";

test("generated image stays decoded and keeps its layout while scrolling", async ({ page }) => {
  const fixturePath = process.env.SIMPLE_MEDIA_FIXTURE_PATH;
  if (fixturePath) {
    const dataUrl = `data:image/png;base64,${(await readFile(fixturePath)).toString("base64")}`;
    await page.addInitScript(value => {
      (window as Window & { __SIMPLE_MEDIA_DATA_URL__?: string }).__SIMPLE_MEDIA_DATA_URL__ = value;
    }, dataUrl);
  }
  await page.goto("/tests/ui/media-render.html");
  const image = page.getByRole("img", { name: "生成图片" });
  await expect(image).toBeVisible();
  await expect(image).toHaveJSProperty("complete", true);
  await expect.poll(() => image.evaluate((node: HTMLImageElement) => node.naturalWidth)).toBeGreaterThan(0);
  await expect.poll(() => image.evaluate((node: HTMLImageElement) => node.naturalHeight)).toBeGreaterThan(0);
  await expect(image).toHaveAttribute("src", /^blob:/);
  await expect(image).toHaveAttribute("width", "1152");
  await expect(image).toHaveAttribute("height", "864");
  await expect(image).toHaveAttribute("loading", "eager");

  const viewport = page.getByTestId("media-scroll");
  const initialHeight = await image.evaluate(node => node.getBoundingClientRect().height);
  expect(initialHeight).toBeGreaterThan(0);
  for (let index = 0; index < 12; index += 1) {
    await viewport.evaluate((node, top) => { node.scrollTop = top; }, index % 2 ? 0 : 900);
    await page.evaluate(() => new Promise(requestAnimationFrame));
    expect(await image.evaluate(node => node.getBoundingClientRect().height)).toBe(initialHeight);
    expect(await image.evaluate((node: HTMLImageElement) => node.naturalWidth)).toBeGreaterThan(0);
  }

  const originalSrc = await image.getAttribute("src");
  await page.getByRole("button", { name: "隐藏" }).click();
  await page.getByRole("button", { name: "显示" }).click();
  const remounted = page.getByRole("img", { name: "生成图片" });
  await expect(remounted).toHaveAttribute("src", originalSrc!);
  expect(await page.evaluate(() => (window as Window & { __SIMPLE_MEDIA_READS__?: number }).__SIMPLE_MEDIA_READS__)).toBe(1);
});

test("generated video keeps a stable frame and cached source while scrolling", async ({ page }) => {
  const fixturePath = process.env.SIMPLE_VIDEO_FIXTURE_PATH;
  test.skip(!fixturePath, "set SIMPLE_VIDEO_FIXTURE_PATH to a real generated MP4");
  const bytes = (await readFile(fixturePath!)).toString("base64");
  await page.addInitScript(value => {
    (window as Window & { __SIMPLE_MEDIA_VIDEO__?: string }).__SIMPLE_MEDIA_VIDEO__ = value;
  }, bytes);
  await page.goto("/tests/ui/media-render.html");

  const video = page.getByLabel("生成视频");
  await expect(video).toBeVisible();
  await expect.poll(() => video.evaluate((node: HTMLVideoElement) => node.readyState)).toBeGreaterThanOrEqual(1);
  await expect.poll(() => video.evaluate((node: HTMLVideoElement) => node.videoWidth)).toBeGreaterThan(0);
  await expect(video).toHaveAttribute("src", /^blob:/);
  await expect(video).toHaveAttribute("preload", "auto");
  await expect(video).toHaveAttribute("playsinline", "");
  await expect(video).toHaveAttribute("poster", /^data:image\/jpeg;base64,/);
  const fallback = page.locator(".stored-video-poster");
  await expect(fallback).toBeVisible();

  const viewport = page.getByTestId("media-scroll");
  const initial = await video.evaluate((node: HTMLVideoElement) => ({
    width: node.getBoundingClientRect().width,
    height: node.getBoundingClientRect().height,
    videoWidth: node.videoWidth,
    videoHeight: node.videoHeight,
  }));
  expect(initial.height).toBeGreaterThan(0);
  for (let index = 0; index < 12; index += 1) {
    await viewport.evaluate((node, top) => { node.scrollTop = top; }, index % 2 ? 0 : 1400);
    await page.evaluate(() => new Promise(requestAnimationFrame));
    expect(await video.evaluate(node => node.getBoundingClientRect().height)).toBe(initial.height);
    expect(await video.evaluate((node: HTMLVideoElement) => node.videoWidth)).toBe(initial.videoWidth);
    expect(await video.evaluate((node: HTMLVideoElement) => node.videoHeight)).toBe(initial.videoHeight);
  }

  await expect(video).toHaveAttribute("width", String(initial.videoWidth));
  await expect(video).toHaveAttribute("height", String(initial.videoHeight));
  const originalSrc = await video.getAttribute("src");
  const originalPoster = await video.getAttribute("poster");
  await page.getByRole("button", { name: "隐藏" }).click();
  await page.getByRole("button", { name: "显示" }).click();
  const remounted = page.getByLabel("生成视频");
  await expect(remounted).toHaveAttribute("src", originalSrc!);
  await expect(remounted).toHaveAttribute("width", String(initial.videoWidth));
  await expect(remounted).toHaveAttribute("height", String(initial.videoHeight));
  await expect(remounted).toHaveAttribute("poster", originalPoster!);
  await expect(page.locator(".stored-video-poster")).toHaveAttribute("src", originalPoster!);
  expect(await page.evaluate(() => (window as Window & { __SIMPLE_VIDEO_READS__?: number }).__SIMPLE_VIDEO_READS__)).toBe(1);
});
