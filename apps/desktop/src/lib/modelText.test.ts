import { describe, expect, it } from "vitest";
import { failureKey, formatBytes, formatEta, progressLine } from "./modelText";

describe("model download text", () => {
  it("shows sizes the way Windows does, in the UI language", () => {
    expect(formatBytes(2_536_786_016, "tr")).toBe("2,4 GB");
    expect(formatBytes(2_536_786_016, "en")).toBe("2.4 GB");
    expect(formatBytes(52_428_800, "en")).toBe("50 MB");
    expect(formatBytes(900, "en")).toBe("0 MB");
  });

  it("rounds the remaining time up to whole minutes", () => {
    expect(formatEta(190, "en")).toBe("~4 min left");
    expect(formatEta(190, "tr")).toBe("~4 dk kaldı");
    expect(formatEta(30, "en")).toBe("<1 min left");
    expect(formatEta(null, "en")).toBe("");
  });

  it("puts received, total, speed and time in one line", () => {
    const p = { received: 1_181_116_006, total: 2_536_786_016, bytes_per_sec: 8_703_180, eta_secs: 156 };
    expect(progressLine(p, "en")).toBe("1.1 GB / 2.4 GB · 8.3 MB/s · ~3 min left");
    expect(progressLine(p, "tr")).toBe("1,1 GB / 2,4 GB · 8,3 MB/sn · ~3 dk kaldı");
  });

  it("maps every failure kind to a message", () => {
    for (const k of ["network", "host", "https", "space", "checksum", "io"]) expect(failureKey(k)).toBe(`model.error.${k}`);
    expect(failureKey("something new")).toBe("model.error.io");
  });
});
