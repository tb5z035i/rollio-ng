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

// `ROLLIO_NATIVE_TARGET` opts the build into a Rust cross-target triple. The
// surrounding workspace's `.cargo/config.toml` carries the linker / CC / bindgen
// wiring for `aarch64-unknown-linux-gnu`; this script just forwards the
// `--target` flag and resolves the artifact path under
// `target/<triple>/release/`.  When unset, the original host-arch behavior is
// preserved.
const rustTarget = process.env.ROLLIO_NATIVE_TARGET || "";

const cargoArgs = ["build", "--release", "--manifest-path", manifestPath];
if (rustTarget) {
  cargoArgs.push("--target", rustTarget);
}

const buildResult = spawnSync("cargo", cargoArgs, {
  cwd: addonDir,
  encoding: "utf8",
});

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
  // The addon's crate-type is `cdylib`, so cargo emits a host-OS-conventional
  // shared library regardless of cross target. Selecting by the Rust target's
  // OS prefix keeps cross compiles correct (e.g. an aarch64-linux-gnu cross
  // produces a `.so` even when run from a darwin host build script).
  const targetOs = rustTarget ? targetOsFromTriple(rustTarget) : process.platform;
  switch (targetOs) {
    case "win32":
    case "windows":
      return "rollio_native_ascii_addon.dll";
    case "darwin":
    case "macos":
      return "librollio_native_ascii_addon.dylib";
    default:
      return "librollio_native_ascii_addon.so";
  }
}

function targetOsFromTriple(triple) {
  if (triple.includes("-windows")) return "windows";
  if (triple.includes("-darwin") || triple.includes("-apple")) return "macos";
  return "linux";
}

const cargoTargetSubdir = rustTarget ? path.join("target", rustTarget) : "target";
const builtAddonPath = path.join(addonDir, cargoTargetSubdir, "release", sourceArtifactName());
await mkdir(outputDir, { recursive: true });
await copyFile(builtAddonPath, outputPath);
