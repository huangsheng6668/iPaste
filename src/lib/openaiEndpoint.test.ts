import { describe, expect, it } from "vitest";
import {
  endpointHost,
  isLocalNoauthEndpoint,
  isOpenaiProfileReady,
} from "./openaiEndpoint";
import type { OpenaiOcrProfile } from "../types";

function profile(overrides: Partial<OpenaiOcrProfile> = {}): OpenaiOcrProfile {
  return {
    id: "p1",
    name: "测试接口",
    baseUrl: "https://api.example.com/v1",
    model: "glm-4v-flash",
    apiKey: "••••••••",
    ...overrides,
  };
}

describe("endpointHost", () => {
  it("剥离协议、端口、路径与 userinfo", () => {
    expect(endpointHost("https://api.example.com/v1")).toBe("api.example.com");
    expect(endpointHost("http://127.0.0.1:11434/v1")).toBe("127.0.0.1");
    expect(endpointHost("http://user:pass@localhost:8080")).toBe("localhost");
    expect(endpointHost("localhost:11434")).toBe("localhost");
  });

  it("IPv6 字面量取方括号内的地址", () => {
    expect(endpointHost("http://[::1]:8080/v1")).toBe("::1");
  });
});

describe("isLocalNoauthEndpoint", () => {
  it("与 Rust 侧 is_local_noauth_endpoint 的用例逐条对齐", () => {
    expect(isLocalNoauthEndpoint("http://localhost:11434/v1")).toBe(true);
    expect(isLocalNoauthEndpoint("http://127.0.0.1:1234/v1")).toBe(true);
    expect(isLocalNoauthEndpoint("http://127.9.9.9")).toBe(true);
    expect(isLocalNoauthEndpoint("http://[::1]:8080/v1")).toBe(true);
    expect(isLocalNoauthEndpoint("http://0.0.0.0:8000")).toBe(true);
    expect(isLocalNoauthEndpoint("https://ollama.localhost/v1")).toBe(true);

    expect(isLocalNoauthEndpoint("https://api.openai.com/v1")).toBe(false);
    expect(isLocalNoauthEndpoint("http://10.0.0.5:11434/v1")).toBe(false);
    expect(isLocalNoauthEndpoint("http://172.16.0.1/v1")).toBe(false);
    expect(isLocalNoauthEndpoint("https://open.bigmodel.cn/api/coding/paas/v4")).toBe(false);
  });
});

describe("isOpenaiProfileReady", () => {
  it("远端接口必须有 Key 才算就绪", () => {
    expect(isOpenaiProfileReady(profile())).toBe(true);
    expect(isOpenaiProfileReady(profile({ apiKey: "" }))).toBe(false);
    expect(isOpenaiProfileReady(profile({ apiKey: "   " }))).toBe(false);
  });

  it("本地无鉴权端点留空 Key 仍算就绪", () => {
    expect(
      isOpenaiProfileReady(
        profile({ baseUrl: "http://127.0.0.1:11434/v1", model: "qwen2.5-vl", apiKey: "" }),
      ),
    ).toBe(true);
  });

  it("地址或模型缺失一律不就绪", () => {
    expect(isOpenaiProfileReady(profile({ baseUrl: "  " }))).toBe(false);
    expect(isOpenaiProfileReady(profile({ model: "" }))).toBe(false);
  });
});
