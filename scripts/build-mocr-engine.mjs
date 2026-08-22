// mocr_engine sidecar 构建编排：Windows 与 macOS Apple Silicon (arm64) 上 cargo build + 拷贝到 externalBin
// 布局；Intel Mac 与 Linux 等平台跳过（走占位与系统 OCR / Python 回退）。由 beforeBuildCommand 的 build:all 调用。
import { spawnSync } from "node:child_process";
import process from "node:process";

const isSupported =
  process.platform === "win32" ||
  (process.platform === "darwin" && process.arch === "arm64");

if (!isSupported) {
  console.log("build:mocr-engine: non-supported platform, skipping");
  process.exit(0);
}

const result = spawnSync(
  "cargo",
  ["build", "--release", "--manifest-path", "src-tauri/Cargo.toml", "--bin", "mocr_engine"],
  { stdio: "inherit", shell: true },
);
if (result.status !== 0) {
  process.exit(result.status ?? 1);
}

const copy = spawnSync("node", ["scripts/copy-mocr-sidecar.mjs"], { stdio: "inherit" });
process.exit(copy.status ?? 1);
