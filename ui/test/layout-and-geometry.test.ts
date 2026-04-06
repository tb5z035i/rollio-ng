import assert from "node:assert/strict";
import test from "node:test";
import sharp from "sharp";
import { resolveCameraNames } from "../src/lib/camera-layout.js";
import { metricsFromWinsize } from "../src/lib/terminal-geometry.js";
import {
  createAsciiRendererBackend,
  nextAsciiRendererId,
} from "../src/lib/renderers/index.js";
import { prepareRendererRaster } from "../src/components/StreamPanel.js";

test("resolveCameraNames keeps configured streams visible", () => {
  assert.deepEqual(
    resolveCameraNames(
      ["camera_d435i_rgb", "camera_d435i_depth"],
      ["camera_d435i_rgb"],
    ),
    ["camera_d435i_rgb", "camera_d435i_depth"],
  );
});

test("resolveCameraNames appends unexpected active streams", () => {
  assert.deepEqual(
    resolveCameraNames(["camera_a"], ["camera_a", "camera_b"]),
    ["camera_a", "camera_b"],
  );
});

test("metricsFromWinsize uses tty pixel geometry when available", () => {
  const metrics = metricsFromWinsize(80, 24, {
    rows: 40,
    cols: 100,
    xpixel: 1000,
    ypixel: 2000,
  });

  assert.equal(metrics.columns, 100);
  assert.equal(metrics.rows, 40);
  assert.equal(metrics.cellGeometry.pixelWidth, 10);
  assert.equal(metrics.cellGeometry.pixelHeight, 50);
});

test("ts-harri defaults to a 1:2 cell aspect ratio", () => {
  const backend = createAsciiRendererBackend("ts-harri");
  const raster = backend.describeRaster({ columns: 4, rows: 3 });
  assert.deepEqual(raster, { width: 32, height: 48 });
});

test("ts-harri honors custom terminal cell geometry", () => {
  const backend = createAsciiRendererBackend("ts-harri", {
    cellGeometry: { pixelWidth: 1, pixelHeight: 3 },
  });
  const raster = backend.describeRaster({ columns: 2, rows: 2 });
  assert.deepEqual(raster, { width: 16, height: 48 });
});

test("nextAsciiRendererId cycles the available camera renderers", () => {
  assert.equal(nextAsciiRendererId("ts-half-block"), "ts-harri");
  assert.equal(nextAsciiRendererId("ts-harri"), "ts-half-block");
});

test("prepareRendererRaster expands single-channel previews to RGB", async () => {
  const encoded = await sharp(Buffer.from([32, 224]), {
    raw: {
      width: 2,
      height: 1,
      channels: 1,
    },
  })
    .png()
    .toBuffer();

  const raster = await prepareRendererRaster(encoded, 2, 1);

  assert.equal(raster.width, 2);
  assert.equal(raster.height, 1);
  assert.equal(raster.data.length, 2 * 1 * 3);
  assert.equal(raster.data[0], raster.data[1]);
  assert.equal(raster.data[1], raster.data[2]);
  assert.equal(raster.data[3], raster.data[4]);
  assert.equal(raster.data[4], raster.data[5]);
  assert.ok(raster.data[3] > raster.data[0]);
});

test("prepareRendererRaster uses cover sizing so panels fill fully", async () => {
  const encoded = await sharp({
    create: {
      width: 4,
      height: 3,
      channels: 3,
      background: { r: 255, g: 255, b: 255 },
    },
  })
    .png()
    .toBuffer();

  const raster = await prepareRendererRaster(encoded, 8, 2);

  assert.equal(raster.width, 8);
  assert.equal(raster.height, 2);
  assert.ok(raster.data.every((value) => value === 255));
});
