import { afterEach, describe, expect, it, vi } from "vitest";

// ocrLanguages imports i18n, which reads localStorage/document at module top
// level. Vitest runs in the node environment (no jsdom), so stub the browser
// globals before the dynamic import — same approach as src/i18n.test.ts.
vi.stubGlobal("localStorage", {
  getItem: () => null,
  setItem: () => undefined,
  removeItem: () => undefined,
});
vi.stubGlobal("document", {
  documentElement: { lang: "en" },
  createElement: () => ({}),
});

const {
  OCR_LANGUAGE_OPTIONS,
  loadOcrLanguage,
  normalizeOcrLanguage,
  ocrLanguageLabel,
  saveOcrLanguage,
} = await import("./ocrLanguages");

function stubLocalStorage(initial: Record<string, string> = {}) {
  const store = new Map(Object.entries(initial));
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => void store.set(key, value),
    removeItem: (key: string) => void store.delete(key),
  });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("normalizeOcrLanguage", () => {
  it("accepts known ids and rejects everything else", () => {
    expect(normalizeOcrLanguage("auto")).toBe("auto");
    expect(normalizeOcrLanguage("ja")).toBe("ja");
    expect(normalizeOcrLanguage("zh-Hant")).toBe("zh-Hant");
    expect(normalizeOcrLanguage("korean")).toBeNull();
    expect(normalizeOcrLanguage(null)).toBeNull();
  });
});

describe("loadOcrLanguage / saveOcrLanguage", () => {
  it("falls back to auto for missing or invalid stored values", () => {
    stubLocalStorage();
    expect(loadOcrLanguage()).toBe("auto");

    stubLocalStorage({ "ipaste.ocrLanguage": "korean" });
    expect(loadOcrLanguage()).toBe("auto");
  });

  it("round-trips a saved language", () => {
    stubLocalStorage();
    saveOcrLanguage("ja");
    expect(loadOcrLanguage()).toBe("ja");
  });
});

describe("ocrLanguageLabel", () => {
  it("maps option ids and engine composite strings", () => {
    expect(ocrLanguageLabel("auto")).toBe(ocrLanguageLabel("auto"));
    const labels = new Set(OCR_LANGUAGE_OPTIONS.map((option) => ocrLanguageLabel(option.id)));
    expect(labels.size).toBe(OCR_LANGUAGE_OPTIONS.length);
    expect(ocrLanguageLabel("zh-Hans+en")).not.toBe("zh-Hans+en");
    expect(ocrLanguageLabel("ja+zh+en")).not.toBe("ja+zh+en");
    expect(ocrLanguageLabel("ja")).not.toBe("ja");
  });

  it("returns unknown strings as-is", () => {
    expect(ocrLanguageLabel("xx-pirate")).toBe("xx-pirate");
  });
});
