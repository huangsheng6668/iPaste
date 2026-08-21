// 拷贝 cargo 构建的 mocr_engine sidecar 到 tauri externalBin 布局：
// src-tauri/binaries/mocr_engine-<host-triple>[.exe]
// 由 `npm run build:mocr-engine` 在 beforeBuildCommand 里调用（tauri build 打包前）。
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");
const manifestDir = path.join(root, "src-tauri");

function hostTriple() {
  const output = execFileSync("rustc", ["-Vv"], { encoding: "utf8" });
  const line = output.split(/\r?\n/).find((l) => l.startsWith("host:"));
  if (!line) throw new Error("rustc -Vv did not report host triple");
  return line.slice("host:".length).trim();
}

const triple = hostTriple();
const ext = process.platform === "win32" ? ".exe" : "";
const source = path.join(manifestDir, "target", "release", `mocr_engine${ext}`);
const targetDir = path.join(manifestDir, "binaries");
const target = path.join(targetDir, `mocr_engine-${triple}${ext}`);

mkdirSync(targetDir, { recursive: true });
copyFileSync(source, target);
console.log(`mocr sidecar: ${source} -> ${target}`);
