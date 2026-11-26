import { execFileSync } from "node:child_process";
import { mkdtemp, readdir, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

// 三平台矩阵并发构建时，tauri-action 的 includeUpdaterJson 会让每个 job 各自
// 对 release 的 latest.json 做读-改-写：
//   - 后完成的 job 可能覆盖掉先完成 job 写入的平台条目（本次 v1.11.2 就只剩
//     windows，macOS 两个平台全部丢失）；
//   - 单个 job 因 runner 网络抖动失败时，release + latest.json 已经被先完成的
//     job 发布出去，用户会拿到一个缺平台、指向不完整版本的更新清单。
//
// 因此改为：矩阵 job 只上传资产（releaseDraft: true），由本脚本作为唯一写入者，
// 收齐并校验全部平台产物后再生成 latest.json、上传、最后把草稿转正。
// 任何平台缺失或签名不带版本都会在这里失败，release 保持草稿，用户不可见。

const repo = process.env.GITHUB_REPOSITORY?.trim();
if (!repo) {
  throw new Error("GITHUB_REPOSITORY is not set.");
}

const packageVersion = JSON.parse(await readFile("package.json", "utf8")).version;
const tag = process.env.RELEASE_TAG?.trim() || `iPaste-v${packageVersion}`;
// 手动补修老版本时（workflow_dispatch 传入 tag），版本以 tag 为准，避免默认分支
// 的 package.json 已经往前走了。
const version = process.env.RELEASE_VERSION?.trim() || tag.replace(/^iPaste-v/, "");
const dryRun = process.env.FINALIZE_DRY_RUN === "1";
const assetDirOverride = process.env.RELEASE_ASSET_DIR?.trim();

const targets = [
  ["darwin-aarch64", "iPaste_aarch64.app.tar.gz"],
  ["darwin-aarch64-app", "iPaste_aarch64.app.tar.gz"],
  ["darwin-x86_64", "iPaste_x64.app.tar.gz"],
  ["darwin-x86_64-app", "iPaste_x64.app.tar.gz"],
  ["windows-x86_64", `iPaste_${version}_x64-setup.exe`],
  ["windows-x86_64-nsis", `iPaste_${version}_x64-setup.exe`],
];
const requiredFiles = [...new Set(targets.map(([, file]) => file))];

const assetDir = assetDirOverride || (await downloadAssets(tag));
const filesOnDisk = new Set(await readdir(assetDir));
const missing = requiredFiles.flatMap((file) => [file, `${file}.sig`]).filter((file) => !filesOnDisk.has(file));
if (missing.length > 0) {
  throw new Error(
    `Release ${tag} is missing updater artifact(s): ${missing.join(", ")}. ` +
      "Re-run the failed matrix job, then re-run this job. The release stays a draft until all platforms are present.",
  );
}

const signatures = new Map();
for (const file of requiredFiles) {
  const raw = (await readFile(path.join(assetDir, `${file}.sig`), "utf8")).trim();
  const signedVersion = readSignedVersion(raw);
  if (!signedVersion) {
    throw new Error(
      `${file}.sig has no \`version:\` in its trusted comment (Tauri CLI < 2.11.5). ` +
        "requireSignedVersion would reject this update for every installed user.",
    );
  }
  if (signedVersion.replace(/^v/, "") !== version) {
    throw new Error(`${file}.sig is bound to version ${signedVersion}, expected ${version}.`);
  }
  signatures.set(file, raw);
}

const release = dryRun ? { body: process.env.RELEASE_NOTES ?? "" } : readRelease(tag);
const manifest = {
  version,
  notes: release.body ?? "",
  pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
  platforms: Object.fromEntries(
    targets.map(([platform, file]) => [
      platform,
      {
        signature: signatures.get(file),
        url: `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(file)}`,
      },
    ]),
  ),
};

const manifestPath = path.join(assetDir, "latest.json");
await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`Built ${manifestPath} for ${version}: ${Object.keys(manifest.platforms).join(", ")}`);

if (dryRun) {
  console.log("FINALIZE_DRY_RUN=1, skipping upload and publish.");
} else {
  run("gh", ["release", "upload", tag, manifestPath, "--clobber"]);
  // latest.json 先就位再把草稿转正：发布瞬间用户拿到的就是完整清单。
  run("gh", ["release", "edit", tag, "--draft=false", "--latest"]);
  console.log(`Published ${tag}.`);
}

async function downloadAssets(releaseTag) {
  const dir = await mkdtemp(path.join(tmpdir(), "ipaste-release-"));
  run("gh", ["release", "download", releaseTag, "--dir", dir, "--clobber"]);
  return dir;
}

function readRelease(releaseTag) {
  const output = run("gh", ["release", "view", releaseTag, "--json", "body,isDraft"], { encoding: "utf8" });
  return JSON.parse(output);
}

function readSignedVersion(signatureBase64) {
  const decoded = Buffer.from(signatureBase64, "base64").toString("utf8");
  const trustedComment = decoded
    .split(/\r?\n/)
    .find((line) => line.startsWith("trusted comment: "));
  if (!trustedComment) return null;

  const versionField = trustedComment
    .slice("trusted comment: ".length)
    .split("\t")
    .find((field) => field.startsWith("version:"));
  return versionField ? versionField.slice("version:".length) : null;
}

function run(command, args, options = {}) {
  return execFileSync(command, args, {
    stdio: options.encoding ? ["inherit", "pipe", "inherit"] : "inherit",
    ...options,
  });
}
