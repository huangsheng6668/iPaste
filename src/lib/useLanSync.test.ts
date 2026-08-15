import { describe, it, expect } from "vitest";
import { reactive, ref } from "vue";
import type { LanCategoryReceivedEvent, LanCategorySentEvent, LanSessionInfo, PortConflict } from "../types";

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

describe("LanCategory transfer event models", () => {
  it("category-sent payload carries counts", () => {
    const event: LanCategorySentEvent = { categoryName: "工作", sent: 12, failed: 1 };
    expect(event.categoryName).toBe("工作");
    expect(event.sent).toBe(12);
    expect(event.failed).toBe(1);
  });

  it("category-received payload carries counts", () => {
    const event: LanCategoryReceivedEvent = { categoryName: "工作", count: 12, failed: 0 };
    expect(event.categoryName).toBe("工作");
    expect(event.count).toBe(12);
    expect(event.failed).toBe(0);
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
});
