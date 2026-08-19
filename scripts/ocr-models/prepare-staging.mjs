// 为发布流程重建 ocr-spike/staging：从钉定 commit 下载 5 个模型/字典文件，
// 按 sha256 + 字节数门禁校验后落成 paddle/{fast,best}/ 布局。
// CI（publish-ocr-assets.yml）与本地均可运行：
//   node scripts/ocr-models/prepare-staging.mjs                       # 从 GitHub 下载
//   node scripts/ocr-models/prepare-staging.mjs --models-dir <dir>    # 用本地 rust-paddle-ocr models/
// 哈希来源：scripts/ocr-models/README.md §1（Task 0/4 实测，权威）。
import { createHash } from "node:crypto";
import { mkdir, readFile, stat, writeFile } from "node:fs/promises";
import path from "node:path";

const SOURCE_COMMIT = "2d0a7e582b955cc6627091765560a78776bcce5c";
const RAW_BASE = `https://raw.githubusercontent.com/zibo-chen/rust-paddle-ocr/${SOURCE_COMMIT}/models`;

const FILES = [
  {
    source: "PP-OCRv5_mobile_det_fp16.mnn",
    dest: "paddle/fast/det.mnn",
    bytes: 2_439_100,
    sha256: "617b5228b101275594f96ebb6ae7662fd1618bcf8e84b0ffde1cf3b48e754951",
  },
  {
    source: "PP-OCRv5_mobile_rec_fp16.mnn",
    dest: "paddle/fast/rec.mnn",
    bytes: 8_371_960,
    sha256: "ff03e4204260325eabe9f4eae0ec8cc6b79b8a97a8e38a5292ba69cf02a689fc",
  },
  {
    source: "ppocr_keys_v5.txt",
    dest: "paddle/fast/ppocr_keys_v5.txt",
    bytes: 92_390,
    sha256: "f3ff5ed81ad3c267593fd3f7183528bb12bbaaa3ab05145ea0ac9ffeffbc6efe",
  },
  {
    source: "PP-OCRv5_mobile_det.mnn",
    dest: "paddle/best/det.mnn",
    bytes: 4_760_244,
    sha256: "326f846bb5c903282e116ea089e8796b67921586726cca9457730436a79684c3",
  },
  {
    source: "PP-OCRv5_mobile_rec.mnn",
    dest: "paddle/best/rec.mnn",
    bytes: 16_531_596,
    sha256: "c809800b09263a8d18c678c211e470ffc464cbb33db2e6bde0244766f3feb0db",
  },
];

function parseArgs(argv) {
  const args = { out: "ocr-spike/staging", modelsDir: null };
  for (let index = 0; index < argv.length; index += 1) {
    const value = argv[index];
    if (value === "--out") {
      args.out = argv[++index];
      if (!args.out) fail(`--out requires a directory`);
    } else if (value === "--models-dir") {
      args.modelsDir = argv[++index];
      if (!args.modelsDir) fail(`--models-dir requires a directory`);
    } else {
      fail(`Unknown argument: ${value}`);
    }
  }
  return args;
}

function fail(message) {
  console.error(`prepare-staging: ${message}`);
  process.exit(1);
}

function sha256(buffer) {
  return createHash("sha256").update(buffer).digest("hex");
}

function verify(file, buffer) {
  if (buffer.byteLength !== file.bytes) {
    fail(
      `${file.source}: size mismatch — expected ${file.bytes} bytes, got ${buffer.byteLength}`,
    );
  }
  const actual = sha256(buffer);
  if (actual !== file.sha256) {
    fail(`${file.source}: sha256 mismatch — expected ${file.sha256}, got ${actual}`);
  }
}

async function loadFile(file, modelsDir) {
  if (modelsDir) {
    return readFile(path.join(modelsDir, file.source));
  }

  const response = await fetch(`${RAW_BASE}/${file.source}`);
  if (!response.ok) {
    fail(`Downloading ${file.source}: HTTP ${response.status}`);
  }
  return Buffer.from(await response.arrayBuffer());
}

async function main() {
  const args = parseArgs(process.argv.slice(2));

  for (const file of FILES) {
    const destPath = path.join(args.out, file.dest);
    try {
      const existing = await readFile(destPath);
      if (sha256(existing) === file.sha256) {
        console.log(`ok (cached) ${file.dest}`);
        continue;
      }
    } catch {
      // 不存在则重新获取
    }

    const buffer = await loadFile(file, args.modelsDir);
    verify(file, buffer);
    await mkdir(path.dirname(destPath), { recursive: true });
    await writeFile(destPath, buffer);
    console.log(`ok ${file.dest} (${file.bytes} bytes)`);
  }

  await stat(path.join(args.out, "manifests")).catch(() =>
    mkdir(path.join(args.out, "manifests"), { recursive: true }),
  );
  console.log(`staging ready at ${args.out}`);
}

main().catch((error) => fail(error.stack ?? String(error)));
