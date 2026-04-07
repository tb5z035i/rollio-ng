import { spawnSync } from "node:child_process";
import { copyFile, mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const uiDir = path.dirname(scriptDir);
const addonDir = path.join(uiDir, "native-ascii-addon");
const outputDir = path.join(uiDir, "native");
const outputPath = path.join(outputDir, "rollio-native-ascii.node");
const manifestPath = path.join(addonDir, "Cargo.toml");

const buildResult = spawnSync(
  "cargo",
  ["build", "--release", "--manifest-path", manifestPath],
  {
    cwd: addonDir,
    encoding: "utf8",
  },
);

if (buildResult.error) {
  console.error(
    "rollio-ui: failed to launch Cargo while building the native ASCII renderer.\n" +
      `${buildResult.error.message}\n`,
  );
  process.exit(1);
}

if (buildResult.status !== 0) {
  process.stderr.write(`${buildResult.stdout ?? ""}${buildResult.stderr ?? ""}`);
  process.exit(buildResult.status ?? 1);
}

function sourceArtifactName() {
  switch (process.platform) {
    case "win32":
      return "rollio_native_ascii_addon.dll";
    case "darwin":
      return "librollio_native_ascii_addon.dylib";
    default:
      return "librollio_native_ascii_addon.so";
  }
}

const builtAddonPath = path.join(addonDir, "target", "release", sourceArtifactName());
await mkdir(outputDir, { recursive: true });
await copyFile(builtAddonPath, outputPath);
