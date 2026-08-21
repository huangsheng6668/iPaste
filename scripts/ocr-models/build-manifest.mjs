// Build an iPaste OCR v2 manifest (paddle engine) from a staged models directory.
//
// Staging layout expected under --dir (mirrors the published R2 key structure):
//   <dir>/paddle/fast/{det.mnn, rec.mnn, ppocr_keys_v5.txt}
//   <dir>/paddle/best/{det.mnn, rec.mnn, ppocr_keys_v5.txt}
//
// Usage:
//   node scripts/ocr-models/build-manifest.mjs \
//     --dir ocr-spike/staging \
//     --mode fast \
//     --base-url "https://<ocr-r2-base-url>/" \
//     --engine-version 2.0.0 \
//     [--out ocr-spike/staging/manifests/ipaste-ocr-windows-x64-fast.json]
//
// Output: manifest JSON on stdout (or written to --out) matching the shape
// validated by src-tauri/src/ocr/installer.rs::validate_ocr_manifest and the
// camelCase serde structs in src-tauri/src/models.rs (OcrManifest*).
//
// Contract enforced here (fail fast before publishing):
//   engine.id       = "paddle"                       (installer OCR_ENGINE_ID)
//   engine.mode     = fast|best, must equal --mode   (clean_ocr_mode values)
//   engine.platform = "windows-x64"                  (ocr_platform() on Windows x64)
//   engine.baseUrl  = https://… with trailing "/"    (installer does baseUrl + file.path)
//   files[].role    = det-model | rec-model | charset
//   files[].path    = paddle/{mode}/<name>           (matches installer OCR_MODEL_DIR
//                                                     layout and paddle_model_paths)
//   files[].size    = exact byte count on disk
//   files[].sha256  = lowercase hex
//   archive / installDir / entries omitted entirely  (v2 ships bare files)

import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const ENGINE_ID = "paddle";
const PLATFORM = "windows-x64";
const MODEL_DIR = "paddle";
const CHARSET_FILE = "ppocr_keys_v5.txt";
const MANIFEST_ROLES = [
  { role: "det-model", name: "det.mnn" },
  { role: "rec-model", name: "rec.mnn" },
  { role: "charset", name: CHARSET_FILE },
];

// Manga-OCR（mocr 引擎）：ONNX 三件套；path 固定 mocr/models/<name>，
// 与 mocr_engine sidecar 及 mocr_onnx::installed_model_dir 的布局对齐。
// encoder/decoder 超 GitHub Pages 100MB 单文件上限，发布时经 --override-url
// 直指 Release 扁平资产
const MOCR_ENGINE_ID = "mocr";
const MOCR_MODEL_DIR = "mocr/models";
const MOCR_FILES = [
  "encoder.onnx",
  "decoder.onnx",
  "vocab.txt",
];

const args = parseArgs(process.argv.slice(2));

const dir = args["dir"];
const mode = args["mode"];
const baseUrl = args["base-url"];
const engineVersion = args["engine-version"];
const out = args["out"];
const engine = args["engine"] ?? "paddle";
const isMocr = engine === "mocr";

// --override-url 可出现多次：name=url，为指定文件写入绝对 url 覆盖
//（manga-ocr 主权重超 GitHub Pages 100MB 单文件上限，直指 Release 扁平资产）
const overrideUrls = new Map();
for (let index = 0; index < process.argv.length; index += 1) {
  if (process.argv[index] !== "--override-url") continue;
  const spec = process.argv[index + 1] ?? fail("--override-url requires name=url");
  const eq = spec.indexOf("=");
  if (eq <= 0) fail(`--override-url must look like name=url (got "${spec}")`);
  overrideUrls.set(spec.slice(0, eq), spec.slice(eq + 1));
}
for (const name of overrideUrls.keys()) {
  if (!MOCR_FILES.includes(name)) {
    fail(`--override-url targets unknown mocr file "${name}"`);
  }
}

if (engine !== "paddle" && engine !== "mocr") {
  fail(`--engine must be "paddle" or "mocr" (got "${engine}")`);
}
if (!dir || !baseUrl || !engineVersion || (!isMocr && !mode)) {
  fail("Missing required args: --dir <staging-dir> --mode fast|best --base-url <https://…/> --engine-version <v> [--engine paddle|mocr] [--out <file>]");
}
if (!isMocr && mode !== "fast" && mode !== "best") {
  fail(`--mode must be "fast" or "best" (got "${mode}")`);
}
if (isMocr && mode) {
  fail("--engine mocr does not take --mode");
}
if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(engineVersion)) {
  fail(`--engine-version must look like a semver (got "${engineVersion}")`);
}
const parsedBase = new URL(baseUrl);
if (parsedBase.protocol !== "https:") {
  fail("--base-url must use https:// (installer rejects insecure download URLs)");
}
if (parsedBase.search || parsedBase.hash) {
  fail("--base-url must not include a query string or hash");
}
const normalizedBaseUrl = baseUrl.endsWith("/") ? baseUrl : `${baseUrl}/`;

const manifest = isMocr
  ? {
      engine: {
        id: MOCR_ENGINE_ID,
        version: engineVersion,
        platform: "any",
        baseUrl: normalizedBaseUrl,
        files: MOCR_FILES.map((name) => {
          if (name.includes("/") || name.includes("\\") || name.includes("..")) {
            fail(`Unsafe file name in manifest spec: ${name}`);
          }
          const filePath = path.join(dir, MOCR_MODEL_DIR, name);
          let content;
          try {
            content = readFileSync(filePath);
          } catch (error) {
            fail(`Missing staged mocr file: ${filePath} (${error.message})`);
          }
          return {
            role: "model",
            name,
            path: `${MOCR_MODEL_DIR}/${name}`,
            size: content.length,
            sha256: createHash("sha256").update(content).digest("hex"),
            ...(overrideUrls.has(name) ? { url: overrideUrls.get(name) } : {}),
          };
        }),
      },
    }
  : {
      engine: {
        id: ENGINE_ID,
        version: engineVersion,
        platform: PLATFORM,
        mode,
        baseUrl: normalizedBaseUrl,
        files: MANIFEST_ROLES.map(({ role, name }) => {
          if (name.includes("/") || name.includes("\\") || name.includes("..")) {
            fail(`Unsafe file name in manifest spec: ${name}`);
          }
          const filePath = path.join(dir, MODEL_DIR, mode, name);
          let content;
          try {
            content = readFileSync(filePath);
          } catch (error) {
            fail(`Missing staged file for role "${role}": ${filePath} (${error.message})`);
          }
          return {
            role,
            name,
            path: `${MODEL_DIR}/${mode}/${name}`,
            size: content.length,
            sha256: createHash("sha256").update(content).digest("hex"),
          };
        }),
      },
    };

const json = `${JSON.stringify(manifest, null, 2)}\n`;
if (out) {
  writeFileSync(out, json);
  console.error(`Wrote manifest for engine "${engine}"${isMocr ? "" : ` mode "${mode}"`} (${manifest.engine.files.length} files) to ${out}`);
} else {
  process.stdout.write(json);
}

function parseArgs(argv) {
  const parsed = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    if (!key.startsWith("--") || index + 1 >= argv.length) {
      fail(`Bad argument at position ${index}: ${key}`);
    }
    parsed[key.slice(2)] = argv[index + 1];
  }
  return parsed;
}

function fail(message) {
  console.error(`build-manifest: ${message}`);
  process.exit(1);
}
