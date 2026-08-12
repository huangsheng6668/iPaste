import { describe, it, expect } from "vitest";
import { reactive, ref } from "vue";
import type { LanSessionInfo, PortConflict } from "../types";

describe("LanSessionInfo reactive model", () => {
  it("Object.assign merges status changes", () => {
    const info = reactive<LanSessionInfo>({
      role: null, status: "idle", code: null, listenAddr: null, peerDeviceName: null,
    });
    Object.assign(info, { status: "connected", peerDeviceName: "MBP" });
    expect(info.status).toBe("connected");
    expect(info.peerDeviceName).toBe("MBP");
  });

  it("LanClipSource current variant shape", () => {
    const source = { kind: "current" as const };
    expect(source.kind).toBe("current");
  });

  it("LanClipSource item variant shape", () => {
    const source = { kind: "item" as const, id: "abc" };
    expect(source.id).toBe("abc");
  });
});

describe("PortConflict reactive model", () => {
  it("holds pid and name", () => {
    const conflict = ref<PortConflict | null>({ pid: 52276, name: "ipaste.exe" });
    expect(conflict.value?.pid).toBe(52276);
    expect(conflict.value?.name).toBe("ipaste.exe");
    conflict.value = null;
    expect(conflict.value).toBeNull();
  });

  it("matches the backend port-in-use error format", () => {
    // 后端格式：`端口 45130 被 ipaste.exe（PID 52276）占用。{bind 原因}`
    // 正则不能带 `$`——错误尾巴还附带了 bind 原因。
    const message = "端口 45130 被 ipaste.exe（PID 52276）占用。Address already in use";
    const m = message.match(/端口 (\d+) 被 (.+?)（PID (\d+)）占用/);
    expect(m).not.toBeNull();
    expect(m?.[2]).toBe("ipaste.exe");
    expect(Number(m?.[3])).toBe(52276);
  });
});
