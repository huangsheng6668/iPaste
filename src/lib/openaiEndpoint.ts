// OpenAI 兼容端点的「是否需要 API Key」判定。
// 口径与 src-tauri/src/ocr/openai.rs 的 `is_local_noauth_endpoint` 逐条对齐：
// 本地无鉴权端点（Ollama / vLLM / LM Studio 等）允许留空 Key，远端端点缺 Key
// 的请求必然被服务端以鉴权错误拒绝。两端判定必须一致，否则会出现「设置页说
// 可用、识别时才报鉴权失败」的错位。
import type { OpenaiOcrProfile } from "../types";

/** base_url → host（丢弃协议、userinfo、端口与 IPv6 方括号），与 Rust 实现同构。 */
export function endpointHost(baseUrl: string): string {
  const trimmed = baseUrl.trim();
  const schemeIndex = trimmed.indexOf("://");
  const rest = schemeIndex >= 0 ? trimmed.slice(schemeIndex + 3) : trimmed;
  const authority = rest.split("/")[0] ?? "";
  // userinfo 可能含 '@'，取最后一个 '@' 之后的部分为 host 段。
  const hostPart = authority.slice(authority.lastIndexOf("@") + 1);
  const host = hostPart.startsWith("[")
    ? (hostPart.slice(1).split("]")[0] ?? "")
    : (hostPart.split(":")[0] ?? "");
  return host.replace(/^\[+/, "").replace(/\]+$/, "").toLowerCase();
}

/** 本地无鉴权端点：允许留空 API Key。 */
export function isLocalNoauthEndpoint(baseUrl: string): boolean {
  const host = endpointHost(baseUrl);
  return (
    host === "localhost" ||
    host.endsWith(".localhost") ||
    host.startsWith("127.") ||
    host === "::1" ||
    host === "0.0.0.0"
  );
}

/**
 * 接口是否可直接用于识别：地址与模型齐备，且远端端点已存 API Key。
 * 前端只持有掩码哨兵——apiKey 非空即代表凭据库里已存凭据，空串才代表缺 Key。
 */
export function isOpenaiProfileReady(profile: OpenaiOcrProfile): boolean {
  if (!profile.baseUrl.trim() || !profile.model.trim()) return false;
  if (profile.apiKey.trim()) return true;
  return isLocalNoauthEndpoint(profile.baseUrl);
}
