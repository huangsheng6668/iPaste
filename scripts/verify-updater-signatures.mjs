import { readFile } from "node:fs/promises";

// 更新器开启了 `requireSignedVersion`（src-tauri/tauri.conf.json），要求每个更新包
// 的 minisign trusted comment 里带 `version:<版本>`，由 Tauri CLI >= 2.11.5 在
// 打包/签名时写入。CLI 更老时签出的 update artifact 不带该字段，已安装用户点更新
// 会直接收到 “更新包校验失败”（MissingSignedVersion），且发布端看起来一切正常。
//
// 该校验在发布工作流里跑在 tauri-action 之后：签名缺 version 或与 tauri.conf 的
// 版本不一致时立刻失败，避免再发一个用户装不上的版本。

const artifactPathsJson = process.env.TAURI_ARTIFACT_PATHS?.trim();
if (!artifactPathsJson) {
  throw new Error("TAURI_ARTIFACT_PATHS is not set.");
}

const packageVersion = JSON.parse(await readFile("package.json", "utf8")).version;
const expectedVersion = (process.env.EXPECTED_VERSION?.trim() || packageVersion).replace(/^v/, "");

const artifactPaths = JSON.parse(artifactPathsJson);
if (!Array.isArray(artifactPaths)) {
  throw new Error("TAURI_ARTIFACT_PATHS must be a JSON array of file paths.");
}

const signatureFiles = artifactPaths.filter((file) => typeof file === "string" && file.endsWith(".sig"));
if (signatureFiles.length === 0) {
  throw new Error(`No updater signature files found in artifact paths: ${artifactPathsJson}`);
}

const failures = [];
for (const file of signatureFiles) {
  const signedVersion = readSignedVersion(await readFile(file, "utf8"));
  if (signedVersion === null) {
    failures.push(`${file}: signature has no \`version:\` in its trusted comment (Tauri CLI < 2.11.5?)`);
  } else if (signedVersion.replace(/^v/, "") !== expectedVersion) {
    failures.push(`${file}: signature is bound to version ${signedVersion}, expected ${expectedVersion}`);
  } else {
    console.log(`OK ${file} -> version ${signedVersion}`);
  }
}

if (failures.length > 0) {
  console.error("\nUpdater signature check failed:");
  for (const failure of failures) console.error(`  - ${failure}`);
  console.error(
    `\nrequireSignedVersion is enabled, so these artifacts cannot be installed by existing users. ` +
      `Upgrade @tauri-apps/cli to >= 2.11.5 and rebuild before publishing ${expectedVersion}.`,
  );
  process.exit(1);
}

console.log(`All ${signatureFiles.length} updater signature(s) are bound to version ${expectedVersion}.`);

function readSignedVersion(signatureBase64) {
  const decoded = Buffer.from(signatureBase64.trim(), "base64").toString("utf8");
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
