import { describe, it, expect } from "vitest";
import { reactive, ref } from "vue";
import type { LanDevice, LanSessionInfo } from "../types";

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

describe("LanDevice reactive model", () => {
  it("scannedDevices list shape", () => {
    const scannedDevices = ref<LanDevice[]>([
      { deviceName: "HostA", addr: "192.168.1.5:45130" },
    ]);
    expect(scannedDevices.value[0].deviceName).toBe("HostA");
    expect(scannedDevices.value[0].addr).toBe("192.168.1.5:45130");
  });
});
