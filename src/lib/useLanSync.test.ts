import { describe, it, expect } from "vitest";
import { reactive } from "vue";
import type { LanSessionInfo } from "../types";

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
