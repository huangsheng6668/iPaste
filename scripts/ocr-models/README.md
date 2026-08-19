# OCR model assets (paddle / PP-OCRv5 mobile)

Operational manual for preparing, verifying, staging, and publishing the
Windows OCR engine assets consumed by the iPaste installer v2
(`src-tauri/src/ocr/installer.rs`). Everything under `ocr-spike/` is
**gitignored local staging**; only this directory (`scripts/ocr-models/`) is
committed.

## 1. Model provenance

No conversion was performed. The `.mnn` models are pre-converted artifacts
taken verbatim from the rust-paddle-ocr repository:

- Source: <https://github.com/zibo-chen/rust-paddle-ocr>
- Pinned commit: `2d0a7e582b955cc6627091765560a78776bcce5c` (2026-08-05,
  verified with `git log -1` on the local clone)
- License verification: the repo's `LICENSE` file at that commit is the
  **Apache License 2.0** text (full license with instructions appendix;
  copyright line is the unfilled template). Redistribution of the model files
  is therefore permitted under Apache-2.0 (keep attribution + license notice —
  this README and the release notes serve that purpose).
- Underlying models: PaddleOCR PP-OCRv5 mobile det/rec, also Apache-2.0
  (PaddlePaddle project).

Re-acquisition (if the local clone is gone):

```bash
git clone https://github.com/zibo-chen/rust-paddle-ocr
cd rust-paddle-ocr && git checkout 2d0a7e582b955cc6627091765560a78776bcce5c
# then copy from models/ per the mapping table below
```

### File manifest (recomputed 2026-08-19, this table is authoritative)

| Published as | Source file in repo `models/` | Bytes | sha256 |
| --- | --- | ---: | --- |
| `paddle/fast/det.mnn` | `PP-OCRv5_mobile_det_fp16.mnn` | 2,439,100 | `617b5228b101275594f96ebb6ae7662fd1618bcf8e84b0ffde1cf3b48e754951` |
| `paddle/fast/rec.mnn` | `PP-OCRv5_mobile_rec_fp16.mnn` | 8,371,960 | `ff03e4204260325eabe9f4eae0ec8cc6b79b8a97a8e38a5292ba69cf02a689fc` |
| `paddle/fast/ppocr_keys_v5.txt` | `ppocr_keys_v5.txt` | 92,390 | `f3ff5ed81ad3c267593fd3f7183528bb12bbaaa3ab05145ea0ac9ffeffbc6efe` |
| `paddle/best/det.mnn` | `PP-OCRv5_mobile_det.mnn` | 4,760,244 | `326f846bb5c903282e116ea089e8796b67921586726cca9457730436a79684c3` |
| `paddle/best/rec.mnn` | `PP-OCRv5_mobile_rec.mnn` | 16,531,596 | `c809800b09263a8d18c678c211e470ffc464cbb33db2e6bde0244766f3feb0db` |
| `paddle/best/ppocr_keys_v5.txt` | `ppocr_keys_v5.txt` | 92,390 | same as fast (identical file) |

Notes:

- `fast` = fp16 quantized, `best` = fp32. Totals match the installer fallback
  constants: fast 10,903,450 B and best 21,384,230 B
  (`OCR_FAST_TOTAL_BYTES` / `OCR_BEST_TOTAL_BYTES`).
- The charset is `ppocr_keys_v5.txt` (18,382 lines) — **not**
  `ppocr_keys_v1.txt`; v1 belongs to PP-OCRv4 and produces wrong output with
  v5 models.

## 2. Smoke verification (2026-08-19, real-screenshot class images)

Images: three dense 1920-px-class renders produced with PowerShell
`System.Drawing` (Task 0's method, scaled up), reproducible via
`ocr-spike/smoke/render-images.ps1`:

- `browser.png` 1920x1080 — browser chrome (address bar with URL) + Chinese
  news article: headline, meta line, 5 wrapped paragraphs
- `editor.png` 1920x1080 — dark-theme code editor: title/menu/tab bars, line
  numbers, 37 lines of syntax-colored Rust with Chinese comments, status bar
- `document.png` 1920x1300 — mixed zh/en product document: title, headings,
  paragraphs, bullet list, bordered table

Run: `ocr-spike/target/release/ocr_spike.exe <models-dir> <image…>` — 3 runs
per image (cold + 2 warm), engine init measured separately.

| Image (items) | Precision | engine_new | runs (cold/warm/warm) |
| --- | --- | ---: | --- |
| browser.png (17) | fp16 (fast) | 7 ms | 1220 / 1175 / 1149 ms |
| browser.png (17) | fp32 (best) | 14 ms | 1273 / 1152 / 1183 ms |
| editor.png (91) | fp16 (fast) | 7 ms | 1037 / 959 / 969 ms |
| editor.png (91) | fp32 (best) | 14 ms | 1056 / 1011 / 1009 ms |
| document.png (34) | fp16 (fast) | 8 ms | 1205 / 1106 / 1135 ms |
| document.png (34) | fp32 (best) | 13 ms | 1089 / 1046 / 1084 ms |

Observations (dev-class Windows x64 desktop, release build):

- Detection counts identical between fp16 and fp32 on all three images;
  recognized text differs on a handful of lines only in trivial ways
  (whitespace around `=`, a single small gutter digit). Sample from
  browser.png fp16, conf 0.987–0.999:
  `https://www.techdaily.cn/ai/2026-08-19/104293.html`,
  `多模态大模型加速落地：2026年产业智能化趋势展望`,
  full wrapped Chinese paragraph lines with correct bboxes.
- Warm per-image time is ~0.96–1.2 s for dense 1920x1080–1300 screenshots;
  fp16 ≈ fp32 here (detection stage dominates). Comfortably below the
  interactive OCR budget.
- On the densest 19 px document lines both precisions occasionally dropped a
  character (e.g. `能力`→`能`, `体积大`→`体积`) at conf ≥ 0.95; coordinates
  stayed sane. Real-world screenshots are usually rendered at larger glyph
  sizes than this stress case.

## 3. Staging layout and regeneration

`ocr-spike/staging/` mirrors the published R2 key structure exactly
(`baseUrl + file.path` must resolve to the uploaded object):

```text
ocr-spike/staging/
├── paddle/
│   ├── fast/  det.mnn  rec.mnn  ppocr_keys_v5.txt   (fp16)
│   └── best/  det.mnn  rec.mnn  ppocr_keys_v5.txt   (fp32)
└── manifests/
    ├── ipaste-ocr-windows-x64-fast.json
    └── ipaste-ocr-windows-x64-best.json
```

Regenerate staging from the spike models, then build both manifests:

```bash
# from the repo/worktree root
mkdir -p ocr-spike/staging/paddle/fast ocr-spike/staging/paddle/best ocr-spike/staging/manifests
cp ocr-spike/models/det.mnn ocr-spike/models/rec.mnn ocr-spike/models/ppocr_keys_v5.txt ocr-spike/staging/paddle/fast/
cp ocr-spike/models/fp32/det.mnn ocr-spike/models/fp32/rec.mnn ocr-spike/models/fp32/ppocr_keys_v5.txt ocr-spike/staging/paddle/best/

node scripts/ocr-models/build-manifest.mjs --dir ocr-spike/staging --mode fast \
  --base-url "$IPASTE_OCR_R2_BASE_URL" --engine-version 2.0.0 \
  --out ocr-spike/staging/manifests/ipaste-ocr-windows-x64-fast.json
node scripts/ocr-models/build-manifest.mjs --dir ocr-spike/staging --mode best \
  --base-url "$IPASTE_OCR_R2_BASE_URL" --engine-version 2.0.0 \
  --out ocr-spike/staging/manifests/ipaste-ocr-windows-x64-best.json
```

The manifests currently staged in `ocr-spike/staging/manifests/` were built
with the placeholder `--base-url https://placeholder.invalid/ocr/` because the
production `IPASTE_OCR_R2_BASE_URL` is a CI secret. **Step 1 of publishing
below regenerates them with the real URL** — sha256/size/path fields are
production-real either way; only `baseUrl` changes.

### Manifest contract (must keep passing `validate_ocr_manifest`)

- `engine.id` = `"paddle"`, `engine.platform` = `"windows-x64"`,
  `engine.mode` = `fast|best` matching the requested mode
- `engine.baseUrl` starts with `https://` and ends with `/` (installer
  concatenates `baseUrl + file.path`)
- `files[].role` ∈ {`det-model`, `rec-model`, `charset`};
  `files[].name` has no `/`, `\`, `..`; `files[].path` = `paddle/{mode}/<name>`
  (also the on-disk layout read back by `paddle_model_paths`)
- `size` exact bytes, `sha256` lowercase hex; `archive` / `installDir` /
  `entries` fields omitted entirely (v2 = bare file downloads)
- JSON field names are camelCase (`baseUrl`) per the serde structs in
  `src-tauri/src/models.rs`

## 4. Publishing procedure — NOT EXECUTED (requires user approval)

Publishes R2 objects + GitHub Release tag `ipaste-ocr-windows-v2`.
**Never delete the v1 assets** (`ipaste-ocr-windows-v1` release and old R2
`ipaste-ocr-*` objects) — rollback depends on them.

Prerequisites (same secrets as `.github/workflows/release.yml` /
`scripts/mirror-r2-release.mjs`): `R2_ACCOUNT_ID`, `R2_BUCKET`,
`R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `IPASTE_OCR_R2_BASE_URL`, and
`gh` authenticated against `iPaste-app/iPaste`.

### Step 1 — regenerate manifests with the production base URL

```bash
node scripts/ocr-models/build-manifest.mjs --dir ocr-spike/staging --mode fast \
  --base-url "$IPASTE_OCR_R2_BASE_URL" --engine-version 2.0.0 \
  --out ocr-spike/staging/manifests/ipaste-ocr-windows-x64-fast.json
node scripts/ocr-models/build-manifest.mjs --dir ocr-spike/staging --mode best \
  --base-url "$IPASTE_OCR_R2_BASE_URL" --engine-version 2.0.0 \
  --out ocr-spike/staging/manifests/ipaste-ocr-windows-x64-best.json
```

### Step 2 — upload to R2 (channel per `scripts/mirror-r2-release.mjs`)

`mirror-r2-release.mjs` drives the AWS CLI against the R2 S3 endpoint with
`R2_*` secrets; it flattens OCR assets by basename, which would destroy the
`paddle/{mode}/` layout — so upload the staged tree directly:

```bash
OCR_KEY_PREFIX="$(node -e 'const u=new URL(process.env.IPASTE_OCR_R2_BASE_URL); console.log(u.pathname.replace(/^\/+|\/+$/g,"")||"ocr")')"
R2_ENDPOINT="https://${R2_ACCOUNT_ID}.r2.cloudflarestorage.com"
export AWS_ACCESS_KEY_ID="$R2_ACCESS_KEY_ID" AWS_SECRET_ACCESS_KEY="$R2_SECRET_ACCESS_KEY" AWS_EC2_METADATA_DISABLED=true

cd ocr-spike/staging
for mode in fast best; do
  for f in det.mnn rec.mnn ppocr_keys_v5.txt; do
    aws --endpoint-url "$R2_ENDPOINT" s3 cp "paddle/$mode/$f" \
      "s3://${R2_BUCKET}/${OCR_KEY_PREFIX}/paddle/$mode/$f" \
      --content-type application/octet-stream \
      --cache-control "public, max-age=31536000, immutable"
  done
  aws --endpoint-url "$R2_ENDPOINT" s3 cp "manifests/ipaste-ocr-windows-x64-$mode.json" \
    "s3://${R2_BUCKET}/${OCR_KEY_PREFIX}/ipaste-ocr-windows-x64-$mode.json" \
    --content-type application/json \
    --cache-control "public, max-age=60"
done
```

This yields objects at `{ocr-prefix}/paddle/{mode}/{file}` — exactly
`baseUrl + file.path` — and manifests at `{ocr-prefix}/ipaste-ocr-windows-x64-{mode}.json`,
the filenames the installer fetches.

### Step 3 — GitHub Release (manifest fallback + archival copies)

GitHub asset names cannot contain `/`, so `paddle/{mode}/…` download paths are
not expressible there. The release therefore hosts (a) the two manifest JSONs
under their exact installer-fetched names — a working manifest fallback, whose
`baseUrl` still points at R2 for the model bytes — and (b) the six model files
under flattened prefixed names for archival:

```bash
DIST="$(mktemp -d)"
cp ocr-spike/staging/manifests/ipaste-ocr-windows-x64-fast.json "$DIST/"
cp ocr-spike/staging/manifests/ipaste-ocr-windows-x64-best.json "$DIST/"
cp ocr-spike/staging/paddle/fast/det.mnn          "$DIST/paddle-fast-det.mnn"
cp ocr-spike/staging/paddle/fast/rec.mnn          "$DIST/paddle-fast-rec.mnn"
cp ocr-spike/staging/paddle/fast/ppocr_keys_v5.txt "$DIST/paddle-fast-ppocr_keys_v5.txt"
cp ocr-spike/staging/paddle/best/det.mnn          "$DIST/paddle-best-det.mnn"
cp ocr-spike/staging/paddle/best/rec.mnn          "$DIST/paddle-best-rec.mnn"
cp ocr-spike/staging/paddle/best/ppocr_keys_v5.txt "$DIST/paddle-best-ppocr_keys_v5.txt"

gh release create ipaste-ocr-windows-v2 "$DIST"/* \
  --repo iPaste-app/iPaste \
  --title "iPaste OCR engine assets v2 (paddle)" \
  --notes "PP-OCRv5 mobile MNN models (fast=fp16, best=fp32) + charset + installer v2 manifests. Source: rust-paddle-ocr @ 2d0a7e5, Apache-2.0; models: PaddleOCR, Apache-2.0. Manifest baseUrl points at R2; these model copies are archival."
```

### Step 4 — verify (read-only)

```bash
for mode in fast best; do
  echo "== $mode =="
  node -e '
    const m = require("./ocr-spike/staging/manifests/ipaste-ocr-windows-x64-" + process.argv[1] + ".json");
    for (const f of m.engine.files) console.log(m.engine.baseUrl + f.path + "  " + f.sha256);
  ' "$mode" | while read -r url expected; do
    actual="$(curl -fsSL "$url" | sha256sum | cut -d" " -f1)"
    [ "$actual" = "$expected" ] && echo "OK   $url" || echo "FAIL $url"
  done
  curl -fsSL "https://github.com/iPaste-app/iPaste/releases/download/ipaste-ocr-windows-v2/ipaste-ocr-windows-x64-$mode.json" >/dev/null \
    && echo "OK   github $mode manifest" || echo "FAIL github $mode manifest"
done
```

## 5. Conversion fallback (recorded, NOT used for this release)

If the pre-converted artifacts ever need regenerating from scratch: take the
official PP-OCRv5 mobile inference models from the PaddleOCR model zoo
(paddlepaddle.org.cn model list) and convert with rust-paddle-ocr's
`script/convert_paddle_to_mnn.py` (requires `pip install paddle2onnx mnn`).
Not exercised here — the repository's pre-converted files are pinned by
sha256 instead.
