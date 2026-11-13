// mocr_engine sidecar 构建编排：Windows 与 macOS 上 cargo build + 拷贝到 externalBin
// 布局。Apple Silicon 产物优先在 ARM64 runner 原生构建；当 GitHub 的 ARM runner
// 池不可用时，可通过 MOCR_BUILD_TARGET=aarch64-apple-darwin 在 Intel runner 上
// 交叉编译（macOS 同 SDK 跨编为官方支持路径）。Intel Mac 本机构建跳过（占位/回退）。
import { spawnSync } from "node:child_process";
import process from "node:process";

const buildTarget = process.env.MOCR_BUILD_TARGET ?? "";
const isSupported =
  process.platform === "win32" ||
  (process.platform === "darwin" &&
    (process.arch === "arm64" || buildTarget === "aarch64-apple-darwin"));

if (!isSupported) {
  console.log("build:mocr-engine: non-supported platform, skipping");
  process.exit(0);
}

const cargoArgs = [
  "build",
  "--release",
  "--manifest-path",
  "src-tauri/Cargo.toml",
  "--bin",
  "mocr_engine",
];
if (buildTarget) cargoArgs.push("--target", buildTarget);

const result = spawnSync("cargo", cargoArgs, { stdio: "inherit", shell: true });
if (result.status !== 0) {
  process.exit(result.status ?? 1);
}

const copy = spawnSync("node", ["scripts/copy-mocr-sidecar.mjs"], { stdio: "inherit" });
process.exit(copy.status ?? 1);
