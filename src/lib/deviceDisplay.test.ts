import { describe, it, expect } from "vitest";
import { cleanTicketInput, fingerprintOf, sortDevices } from "./deviceDisplay";
import type { DeviceInfo } from "../types/generated/DeviceInfo";

function device(
  overrides: Partial<DeviceInfo["device"]> & Pick<DeviceInfo["device"], "nodeId" | "deviceName" | "addedAt">,
  online: DeviceInfo["online"] = "offline",
): DeviceInfo {
  return {
    online,
    device: {
      relayUrl: null,
      directAddrs: [],
      autoSyncMode: "text_only",
      lastSeenAt: null,
      revokedAt: null,
      ...overrides,
    },
  };
}

describe("sortDevices", () => {
  it("orders connected before connecting before offline", () => {
    const offline = device({ nodeId: "a", deviceName: "A", addedAt: "2026-01-01T00:00:00Z" }, "offline");
    const connecting = device({ nodeId: "b", deviceName: "B", addedAt: "2026-01-02T00:00:00Z" }, "connecting");
    const connected = device({ nodeId: "c", deviceName: "C", addedAt: "2026-01-03T00:00:00Z" }, "connected");
    expect(sortDevices([offline, connecting, connected]).map((entry) => entry.device.nodeId)).toEqual([
      "c",
      "b",
      "a",
    ]);
  });

  it("sinks revoked devices below all active ones", () => {
    const active = device({ nodeId: "active", deviceName: "Active", addedAt: "2026-05-01T00:00:00Z" }, "offline");
    const revokedOnline = device(
      { nodeId: "revoked", deviceName: "Revoked", addedAt: "2026-01-01T00:00:00Z", revokedAt: "2026-06-01T00:00:00Z" },
      "connected",
    );
    expect(sortDevices([revokedOnline, active]).map((entry) => entry.device.nodeId)).toEqual(["active", "revoked"]);
  });

  it("breaks ties by addedAt ascending (earlier pairing first)", () => {
    const later = device({ nodeId: "later", deviceName: "Later", addedAt: "2026-03-01T00:00:00Z" }, "offline");
    const earlier = device({ nodeId: "earlier", deviceName: "Earlier", addedAt: "2026-02-01T00:00:00Z" }, "offline");
    expect(sortDevices([later, earlier]).map((entry) => entry.device.nodeId)).toEqual(["earlier", "later"]);
  });

  it("does not mutate the input array", () => {
    const input = [
      device({ nodeId: "b", deviceName: "B", addedAt: "2026-02-01T00:00:00Z" }),
      device({ nodeId: "a", deviceName: "A", addedAt: "2026-01-01T00:00:00Z" }),
    ];
    sortDevices(input);
    expect(input.map((entry) => entry.device.nodeId)).toEqual(["b", "a"]);
  });
});

describe("fingerprintOf", () => {
  it("keeps the first 8 hex chars of a node id", () => {
    expect(fingerprintOf("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")).toBe("01234567");
  });

  it("returns short ids unchanged", () => {
    expect(fingerprintOf("abcd")).toBe("abcd");
  });
});

describe("cleanTicketInput", () => {
  it("trims surrounding whitespace", () => {
    expect(cleanTicketInput("  ipaste-pair-v1:abc  ")).toBe("ipaste-pair-v1:abc");
  });

  it("strips accidental inner whitespace and newlines", () => {
    expect(cleanTicketInput("ipaste-pair-v1:\n a b\tc\r\n")).toBe("ipaste-pair-v1:abc");
  });

  it("returns empty string for whitespace-only input", () => {
    expect(cleanTicketInput(" \n\t ")).toBe("");
  });
});
