import assert from "node:assert/strict";
import test from "node:test";
import sharp from "sharp";
import {
  MAX_PREVIEW_CAMERAS,
  resolveCameraNames,
} from "../src/lib/camera-layout.js";
import { metricsFromWinsize } from "../src/lib/terminal-geometry.js";
import {
  createAsciiRendererBackend,
  nextAsciiRendererId,
} from "../src/lib/renderers/index.js";
import {
  describeCameraPreviewRaster,
  prepareRendererRaster,
} from "../src/components/StreamPanel.js";

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

test(`resolveCameraNames returns every configured channel even when more than ${MAX_PREVIEW_CAMERAS} are configured (overflow wraps to a second row in LivePreviewPanels, not a silent drop)`, () => {
  const configured = Array.from(
    { length: MAX_PREVIEW_CAMERAS + 2 },
    (_, i) => `cam_${i}`,
  );
  const names = resolveCameraNames(configured, []);
  assert.deepEqual(names, configured);
});

test("describeCameraPreviewRaster keeps each tile within the 16:10 visual aspect envelope", () => {
  // 1:2 cell geometry mimics a typical monospace terminal where each cell
  // is twice as tall as it is wide. With three cameras sharing a wide
  // preview row, the cell grid must compensate so each tile *visually*
  // approximates the 16:10 box.
  const cellGeometry = { pixelWidth: 1, pixelHeight: 2 };
  const raster = describeCameraPreviewRaster(
    /* totalWidth */ 240,
    /* panelHeight */ 30,
    /* numCameras */ 3,
    cellGeometry,
    "ts-half-block",
  );
  // visualAspect = (cols * cellPixelW) / (rows * cellPixelH).
  // We tolerate a small rounding gap (the cell counts are integer-valued).
  const visualAspect =
    (raster.columns * cellGeometry.pixelWidth) /
    (raster.rows * cellGeometry.pixelHeight);
  assert.ok(
    Math.abs(visualAspect - 16 / 10) < 0.25,
    `expected ~16:10 visual aspect, got ${visualAspect.toFixed(3)} (cols=${raster.columns}, rows=${raster.rows})`,
  );
});

test("describeCameraPreviewRaster honours the per-camera width budget when height is the limit", () => {
  // Tall, narrow panel: the height-bound layout would emit columns that
  // exceed the per-camera width budget, so the renderer falls back to
  // width-bound sizing while preserving the 16:10 aspect ratio.
  const cellGeometry = { pixelWidth: 1, pixelHeight: 2 };
  const raster = describeCameraPreviewRaster(
    /* totalWidth */ 60,
    /* panelHeight */ 80,
    /* numCameras */ 3,
    cellGeometry,
    "ts-half-block",
  );
  // perCameraColumnsBudget = floor((60 - 2 - 2) / 3) = 18.
  assert.ok(
    raster.columns <= 18,
    `tile columns should respect the per-camera width budget; got ${raster.columns}`,
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

test("native-rust uses engine-backed context-shape raster layout", () => {
  const backend = createAsciiRendererBackend("native-rust");
  assert.deepEqual(backend.describeRaster({ columns: 4, rows: 3 }), {
    width: 32,
    height: 48,
  });
});

test("native-rust-color shares context-shape raster layout", () => {
  const backend = createAsciiRendererBackend("native-rust-color");
  assert.deepEqual(backend.describeRaster({ columns: 4, rows: 3 }), {
    width: 32,
    height: 48,
  });
});

test("native-rust honors custom terminal cell geometry through the engine", () => {
  const backend = createAsciiRendererBackend("native-rust", {
    cellGeometry: { pixelWidth: 1, pixelHeight: 3 },
  });
  assert.deepEqual(backend.describeRaster({ columns: 2, rows: 2 }), {
    width: 16,
    height: 48,
  });
});

test("ts-half-block uses engine-backed half-block raster layout", () => {
  const backend = createAsciiRendererBackend("ts-half-block");
  assert.deepEqual(backend.describeRaster({ columns: 4, rows: 3 }), {
    width: 4,
    height: 6,
  });
  assert.deepEqual(backend.layoutForRaster({ width: 7, height: 9 }), {
    columns: 7,
    rows: 5,
  });
});

test("nextAsciiRendererId cycles the available camera renderers", () => {
  assert.equal(nextAsciiRendererId("native-rust"), "native-rust-color");
  assert.equal(nextAsciiRendererId("native-rust-color"), "ts-half-block");
  assert.equal(nextAsciiRendererId("ts-half-block"), "native-rust");
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

test("prepareRendererRaster emits luma8 for grayscale-native backends", async () => {
  const encoded = await sharp(Buffer.from([16, 240]), {
    raw: {
      width: 2,
      height: 1,
      channels: 1,
    },
  })
    .png()
    .toBuffer();

  const raster = await prepareRendererRaster(encoded, 2, 1, "luma8");

  assert.equal(raster.width, 2);
  assert.equal(raster.height, 1);
  assert.equal(raster.data.length, 2);
  assert.ok(raster.data[1] > raster.data[0]);
});

test("prepareRendererRaster converts color JPEGs to explicit luma8", async () => {
  const encoded = await sharp({
    create: {
      width: 2,
      height: 1,
      channels: 3,
      background: { r: 255, g: 0, b: 0 },
    },
  })
    .jpeg()
    .toBuffer();

  const raster = await prepareRendererRaster(encoded, 2, 1, "luma8");

  assert.equal(raster.width, 2);
  assert.equal(raster.height, 1);
  assert.equal(raster.data.length, 2);
});

test("prepareRendererRaster keeps resized luma8 buffers single-channel", async () => {
  const encoded = await sharp({
    create: {
      width: 4,
      height: 3,
      channels: 3,
      background: { r: 255, g: 128, b: 0 },
    },
  })
    .jpeg()
    .toBuffer();

  const raster = await prepareRendererRaster(encoded, 8, 6, "luma8");

  assert.equal(raster.width, 8);
  assert.equal(raster.height, 6);
  assert.equal(raster.data.length, 8 * 6);
});

test("prepareRendererRaster letterboxes the source so its native aspect ratio is preserved", async () => {
  // 4:3 source rendered into an 8:2 target. With "contain" sizing the
  // image is scaled to height=2 (preserving the 4:3 aspect → 3 px wide) and
  // padded with black on the left / right so it never gets stretched.
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
  // At least one pixel column must remain black (letterbox padding).
  // Pixels are RGB triples in row-major order. We sum the first row's
  // values and look for any zero byte to prove the padding is present.
  const firstRow = raster.data.subarray(0, raster.width * 3);
  assert.ok(
    firstRow.includes(0),
    "expected letterbox padding (zero bytes) in the first row of the contain-fit output",
  );
  // Conversely, at least one pixel must remain at the source colour (255)
  // so we know the actual image content survived the resize unaltered.
  assert.ok(
    firstRow.includes(255),
    "expected source pixels to survive the contain-fit resize",
  );
});
