import appPackage from "../../../package.json";
import type {
  AppInfo,
  AppSettings,
  AppSnapshot,
  Category,
  CategoryItem,
  ClipItem,
  ImageOcrResult,
  OcrInstallStatus,
} from "../../types";
import { DEFAULT_OPENAI_OCR_PROMPTS } from "../../stores/lib/settings";

export { DEFAULT_OPENAI_OCR_PROMPTS };

const fallbackAppInfo: AppInfo = {
  version: appPackage.version,
};
const fallbackOcrInstallStatus: OcrInstallStatus = {
  installed: false,
  engineId: "paddle",
  engineVersion: null,
  mode: "fast",
  platform: "windows-x64",
  manifestUrl: "https://github.com/huangsheng6668/iPaste/releases/download/ipaste-ocr-windows-v2/ipaste-ocr-windows-x64-fast.json",
  installDir: "",
  downloadedBytes: 0,
  totalBytes: 10_885_068,
  missingFiles: [],
};

const fallbackMocrInstallStatus: OcrInstallStatus = {
  installed: false,
  engineId: "mocr",
  engineVersion: null,
  mode: "mocr",
  platform: "windows-x64",
  manifestUrl: "https://github.com/huangsheng6668/iPaste/releases/download/ipaste-ocr-mocr-v1/ipaste-ocr-mocr-v1.json",
  installDir: "",
  downloadedBytes: 0,
  totalBytes: 460_790_482,
  missingFiles: [],
};

const mockCategories: Category[] = [
  {
    id: "dev",
    name: "Dev Snippets",
    color: "#2563EB",
    sortOrder: 0,
    createdAt: new Date(Date.now() - 42_400_000).toISOString(),
    updatedAt: new Date(Date.now() - 42_400_000).toISOString(),
  },
];

const mockClips: ClipItem[] = [
  {
    id: "clip-image",
    clipType: "image",
    contentHash: "mock-image",
    displayName: null,
    previewText: "Image 240 x 160",
    text: "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='240' height='160' viewBox='0 0 240 160'%3E%3Crect width='240' height='160' rx='18' fill='%23dbeafe'/%3E%3Ccircle cx='76' cy='70' r='28' fill='%230d9488'/%3E%3Cpath d='M40 126l48-42 34 30 28-24 50 36z' fill='%232563eb' opacity='.75'/%3E%3C/svg%3E",
    sourceApp: "Preview",
    lastCapturedAt: new Date(Date.now() - 60_000).toISOString(),
    favoriteCount: 0,
    isPinned: false,
  },
  {
    id: "clip-link",
    clipType: "link",
    contentHash: "mock-link",
    displayName: "Tauri clipboard docs",
    previewText: "https://tauri.app/plugin/clipboard/",
    text: "https://tauri.app/plugin/clipboard/",
    sourceApp: "Safari",
    lastCapturedAt: new Date(Date.now() - 120_000).toISOString(),
    favoriteCount: 1,
    isPinned: false,
  },
  {
    id: "clip-color",
    clipType: "color",
    contentHash: "mock-color",
    displayName: null,
    previewText: "#0D9488",
    text: "#0D9488",
    sourceApp: "Figma",
    lastCapturedAt: new Date(Date.now() - 500_000).toISOString(),
    favoriteCount: 0,
    isPinned: false,
  },
  {
    id: "clip-text",
    clipType: "text",
    contentHash: "mock-text",
    displayName: null,
    previewText: "Use Tauri commands for native clipboard capture, keeping Vue state focused on UI interactions.",
    text: "Use Tauri commands for native clipboard capture, keeping Vue state focused on UI interactions.",
    sourceApp: "Notes",
    lastCapturedAt: new Date(Date.now() - 1_100_000).toISOString(),
    favoriteCount: 2,
    isPinned: false,
  },
];

// 凑足两页以上，便于浏览器（无 Tauri）环境下开发调试触底分页加载。
for (let index = 0; index < 26; index += 1) {
  const text = `Mock clipboard entry #${index + 1}`;
  mockClips.push({
    id: `mock-generated-${index}`,
    clipType: "text",
    contentHash: `mock-generated-${index}`,
    displayName: null,
    previewText: text,
    text,
    sourceApp: "Mock",
    lastCapturedAt: new Date(Date.now() - (1_200_000 + index * 60_000)).toISOString(),
    favoriteCount: 0,
    isPinned: false,
  });
}

const mockCategoryItems: CategoryItem[] = [
  {
    id: "saved-text",
    categoryId: "dev",
    clipSnapshotId: "clip-text",
    clipType: "text",
    contentHash: "mock-text",
    displayName: "Tauri state note",
    previewText: "Use Tauri commands for native clipboard capture, keeping Vue state focused on UI interactions.",
    text: "Use Tauri commands for native clipboard capture, keeping Vue state focused on UI interactions.",
    sortOrder: 0,
    createdAt: new Date(Date.now() - 90_000).toISOString(),
    updatedAt: new Date(Date.now() - 90_000).toISOString(),
    syncState: "local",
    isPinned: false,
  },
];

const mockSnapshot: AppSnapshot = {
  clips: mockClips.slice(0, 20),
  hasMoreClips: mockClips.length > 20,
  clipTotalCount: mockClips.length,
  categories: mockCategories,
  categoryItems: mockCategoryItems,
  shortcut: "CommandOrControl+Shift+V",
  isListening: true,
  isAppendCopyEnabled: false,
  settings: {
    shortcut: "CommandOrControl+Shift+V",
    ocrShortcut: "CommandOrControl+Shift+O",
    retentionDays: 30,
    appendCopyTimeoutMinutes: 1,
    panelOpenBehavior: "history",
    panelLayout: "top",
    ocrMode: "fast",
    ocrEngine: "local",
    language: "en",
    cloud: {
      apiAddress: "",
      apiKey: "",
      enabled: false,
      lastConnectedAt: null,
    },
    cloudOcr: {
      openaiBaseUrl: "",
      openaiModel: "",
      openaiApiKey: "",
      openaiPrompts: DEFAULT_OPENAI_OCR_PROMPTS.map((item) => ({ ...item })),
    },
  },
};

function mockSettings(overrides: Partial<AppSettings> = {}): AppSettings {
  return { ...mockSnapshot.settings, ...overrides };
}

/** 浏览器模式 create_category / update_category 的回落分类（update 需保留原 id 供前端按 id 匹配）。 */
function buildMockCategory(name: string, color: string, sortOrder: number, id: string = crypto.randomUUID()): Category {
  const timestamp = new Date().toISOString();
  return { id, name, color, sortOrder, createdAt: timestamp, updatedAt: timestamp };
}

/** 浏览器模式 add_clip_to_category / create_category_with_clip 的回落条目。 */
function buildMockCategoryItem(clip: ClipItem, categoryId: string): CategoryItem {
  const timestamp = new Date().toISOString();
  return {
    id: crypto.randomUUID(),
    categoryId,
    clipSnapshotId: clip.id,
    clipType: clip.clipType,
    contentHash: clip.contentHash,
    displayName: clip.displayName,
    previewText: clip.previewText,
    text: clip.text,
    sortOrder: 0,
    createdAt: timestamp,
    updatedAt: timestamp,
    syncState: "local",
    isPinned: false,
  };
}

/** 浏览器模式 install_ocr_assets 完成态回落。 */
function buildInstalledPaddleStatus(): OcrInstallStatus {
  return {
    ...fallbackOcrInstallStatus,
    installed: true,
    engineVersion: "2.0.0",
    mode: fallbackOcrInstallStatus.mode,
    downloadedBytes: 10_885_068,
    totalBytes: 10_885_068,
  };
}

/** 浏览器模式 install_mocr_assets 完成态回落。 */
function buildInstalledMocrStatus(): OcrInstallStatus {
  return {
    ...fallbackMocrInstallStatus,
    installed: true,
    downloadedBytes: 460_790_482,
  };
}

/** 浏览器模式 recognize_image_text 的演示识别结果。 */
function buildMockImageOcrResult(): ImageOcrResult {
  return {
    text: "iPaste image OCR test Select text from image 2026",
    engine: "paddle",
    language: "zh-Hans+en",
    words: [],
  };
}

export {
  fallbackAppInfo,
  fallbackMocrInstallStatus,
  fallbackOcrInstallStatus,
  buildInstalledMocrStatus,
  buildInstalledPaddleStatus,
  buildMockCategory,
  buildMockCategoryItem,
  buildMockImageOcrResult,
  mockCategories,
  mockCategoryItems,
  mockClips,
  mockSettings,
  mockSnapshot,
};
