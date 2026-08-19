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

### 2.1 Supplement (2026-08-19): real screen captures

The images above are synthetic `System.Drawing` renders. This supplement
re-ran the smoke on **real captures of the physical screen** (the case the
review flagged: actual ClearType subpixel rendering, live desktop furniture,
scale variance). Methodology:

- Primary display 2560x1440, 96 DPI (100% scaling), captured full-screen via
  PowerShell `System.Windows.Forms` + `Graphics::CopyFromScreen`. Reproducible:
  content sources + `capture.ps1` under `ocr-spike/smoke/real/src/`
  (gitignored); raw logs under `ocr-spike/smoke/real/out-*.txt`. Apps were
  launched with isolated temp profiles (`--user-data-dir`) and killed after
  capture; captures include the real taskbar/desktop, as a user screenshot
  would.
- `zh-paragraph.png` — Chrome (100% scale) rendering local
  `src/zh-paragraph.html`: browser chrome + Chinese news article (headline,
  meta line, 5 justified 17 px paragraphs in Microsoft YaHei, pull quote).
- `en-ui.png` — VS Code (fresh English-UI profile) maximized on
  `scripts/ocr-models/build-manifest.mjs` + `README.md`: menu bar,
  breadcrumbs, explorer sidebar, line numbers, dense syntax-colored code,
  status bar, minimap.
- `mixed-zh-en.png` — Chrome with `--force-device-scale-factor=1.5`
  (emulated 150% display scaling — DPI/scale variance) rendering
  `src/mixed-doc.html`: zh+en release notes with bullets and a bordered table.

| Image (items fp16/fp32) | Precision | engine_new | runs (cold/warm/warm) |
| --- | --- | ---: | --- |
| zh-paragraph.png 2560x1440 (82/86) | fp16 (fast) | 7 ms | 1360 / 1454 / 2164 ms |
| zh-paragraph.png | fp32 (best) | 14 ms | 1522 / 1431 / 2215 ms |
| en-ui.png 2560x1440 (140/137) | fp16 (fast) | 13 ms | 2090 / 1952 / 2030 ms |
| en-ui.png | fp32 (best) | 22 ms | 2005 / 2040 / 1973 ms |
| mixed-zh-en.png 2560x1440 (32/32) | fp16 (fast) | 11 ms | 898 / 863 / 829 ms |
| mixed-zh-en.png | fp32 (best) | 23 ms | 941 / 864 / 871 ms |

(Timings taken on the live interactive desktop — warm-run variance such as
zh 2164 ms reflects background load, not the engine.)

Quality vs the synthetic baseline:

- **zh**: body text conf 0.994–0.999 including full-width justified
  paragraphs and the pull quote; browser chrome read correctly (tab title,
  `file:///…zh-paragraph.html` address bar at 0.989). No ClearType-fringe
  garbling on document text. fp16 vs fp32 differ only in tiny desktop marks
  (fp32 picked up 4 more tray/glyph items) and one comma on a small foreign
  window title captured at the screen edge.
- **en**: English UI (File/Edit/View/Go/Run/Terminal/Help) conf 0.99–1.00;
  code lines up to conf 0.998, e.g. `import { createHash } from
  "node:crypto";` at 0.958. Sub-0.90 items are 27–35 px icon furniture
  (sidebar chevrons read as `1`/`11`, activity-bar icons as letters), not
  lost text. Small ~14 px chrome text shows case glitches: `OCR`→`ocR`,
  `JSON`→`JsoN`, `JS`→`Js`.
- **mixed @150%**: fastest and cleanest (0.83–0.94 s) — larger effective
  glyphs from the emulated 150% scaling improve both speed and accuracy;
  full document read end-to-end at conf ≥ 0.94, zh bullets and table cells
  intact. Residual slips on small cells: `DPI`→`DPl`, `ms`→`mS`, one
  `full screen`→`full creen`, and `96 MB` degraded to `M` (conf 0.61). The
  only fp16/fp32 text diff in the document body was a space.
- **fp16 vs fp32**: on real captures item counts differ slightly
  (82/86, 140/137, 32/32) — entirely on tiny/low-contrast UI marks; document
  text is effectively identical. Confirms Task 0's fp16≈fp32 finding under
  real rendering.
- Real vs synthetic: real screens carry more detectable furniture
  (en-ui 140 items vs 91 synthetic) and run longer (en-ui ~2.0 s vs ~1.0 s
  synthetic) yet stay within the interactive OCR budget. PNG captures are
  lossless, so no compression artifacts were exercised; the small-text case
  glitches above (`l`/`I`/`s`/`S`) are the realistic residual error mode.

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

**Release gate.** No Windows app release may be cut from this branch until
step 2 has put the v2 manifests live on R2: a released build without the
published v2 assets loses OCR entirely (the installer rejects the old v1
manifests via `validate_ocr_manifest`, and the paddle model objects do not
exist at any reachable URL). During publish-day QA, the
`IPASTE_OCR_R2_BASE_URL` runtime override (read in `ocr_r2_base_urls`,
`src-tauri/src/ocr/installer.rs`) points a dev build at staging to preview
exactly what production will serve.

Prerequisites (same secrets as `.github/workflows/release.yml` /
`scripts/mirror-r2-release.mjs`): `R2_ACCOUNT_ID`, `R2_BUCKET`,
`R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `IPASTE_OCR_R2_BASE_URL`, and
`gh` authenticated against the release repository (`huangsheng6668/iPaste`,
same as the tauri.conf.json updater endpoint). The automated path is
`.github/workflows/publish-ocr-assets.yml` (workflow_dispatch + push to main
touching `scripts/ocr-models/**`), which runs this procedure in CI.

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
  --repo huangsheng6668/iPaste \
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
  curl -fsSL "https://github.com/huangsheng6668/iPaste/releases/download/ipaste-ocr-windows-v2/ipaste-ocr-windows-x64-$mode.json" >/dev/null \
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
