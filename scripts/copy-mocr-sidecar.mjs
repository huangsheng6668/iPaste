// 拷贝 cargo 构建的 mocr_engine sidecar 到 tauri externalBin 布局：
// src-tauri/binaries/mocr_engine-<triple>[.exe]（triple = 构建目标，交叉编译时
// 来自 MOCR_BUILD_TARGET，否则取 host triple）。
// 由 `npm run build:mocr-engine` 在 beforeBuildCommand 里调用（tauri build 打包前）。
// 支持 Windows 与 macOS（含 Intel runner 交叉编译 aarch64）；其余平台跳过。
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import path from "node:path";

const buildTarget = process.env.MOCR_BUILD_TARGET ?? "";
const isSupported =
  process.platform === "win32" ||
  (process.platform === "darwin" &&
    (process.arch === "arm64" || buildTarget === "aarch64-apple-darwin"));

if (!isSupported) {
  console.log("mocr sidecar: non-supported platform, skipping");
  process.exit(0);
}

const root = path.resolve(import.meta.dirname, "..");
const manifestDir = path.join(root, "src-tauri");

function hostTriple() {
  const output = execFileSync("rustc", ["-Vv"], { encoding: "utf8" });
  const line = output.split(/\r?\n/).find((l) => l.startsWith("host:"));
  if (!line) throw new Error("rustc -Vv did not report host triple");
  return line.slice("host:".length).trim();
}

const triple = buildTarget || hostTriple();
const ext = process.platform === "win32" ? ".exe" : "";
// 交叉编译时 cargo 产物位于 target/<triple>/release/ 下
const releaseDir = buildTarget
  ? path.join("target", buildTarget, "release")
  : path.join("target", "release");
const source = path.join(manifestDir, releaseDir, `mocr_engine${ext}`);
const targetDir = path.join(manifestDir, "binaries");
const target = path.join(targetDir, `mocr_engine-${triple}${ext}`);

mkdirSync(targetDir, { recursive: true });
copyFileSync(source, target);
console.log(`mocr sidecar: ${source} -> ${target}`);
