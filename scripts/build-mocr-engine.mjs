// mocr_engine sidecar 构建编排：Windows 上 cargo build + 拷贝到 externalBin
// 布局；非 Windows 平台直接跳过（macOS 的 ort 编译问题解决前不分发 sidecar，
// mocr 识别走 Python/Paddle 回退）。由 beforeBuildCommand 的 build:all 调用。
import { spawnSync } from "node:child_process";
import process from "node:process";

if (process.platform !== "win32") {
  console.log("build:mocr-engine: non-Windows platform, skipping");
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
