// 共享契约类型：结构字段来自 src/types/generated（ts-rs 从 Rust 生成，
// 运行 npm run gen:types 再生成）。本文件只做三件事：
// 1. 再导出生成类型（保持既有类型名不变）；
// 2. 用 Omit & 交集恢复前端的窄字面量联合（Rust 侧仍是 String 的字段）；
// 3. 定义纯前端类型（ClipViewItem、ClipUpdatedEvent 等）。
// 不允许在本文件手写与 Rust 重复的结构字段。
//
// Task 40 起，panelOpenBehavior/panelLayout/ocrMode/ocrEngine/language 在 Rust 侧已是 serde 枚举，
// 生成物自带字面量联合，这里不再手写同名联合，直接再导出（Rust 增删枚举值时前端自动跟随）。

export type ClipType = "text" | "link" | "color" | "image" | "file" | "html";
/** 历史面板的类型筛选："text" 涵盖一切可搜索的文本类（text/link/color/html/file），"image" 仅图片。 */
export type ClipTypeFilter = "all" | "text" | "image";
export type AutomationStatus = "idle" | "running" | "success" | "failed" | "timed_out";
export type SyncState = "local" | "syncing" | "synced" | "conflict";

import type { ClipItem as ClipItemGen } from "./types/generated/ClipItem";
import type { CategoryItem as CategoryItemGen } from "./types/generated/CategoryItem";
import type { AppSnapshot as AppSnapshotGen } from "./types/generated/AppSnapshot";
import type { ClipPage as ClipPageGen } from "./types/generated/ClipPage";
import type { CategoryWithItem as CategoryWithItemGen } from "./types/generated/CategoryWithItem";
import type { CategoryHitGroup as CategoryHitGroupGen } from "./types/generated/CategoryHitGroup";
import type { SearchResult as SearchResultGen } from "./types/generated/SearchResult";
import type { AppSettings } from "./types/generated/AppSettings";
import type { Language } from "./types/generated/Language";
import type { OcrEngine } from "./types/generated/OcrEngine";
import type { OcrMode } from "./types/generated/OcrMode";
import type { PanelLayout } from "./types/generated/PanelLayout";
import type { PanelOpenBehavior } from "./types/generated/PanelOpenBehavior";
import type { OcrInstallStatus as OcrInstallStatusGen } from "./types/generated/OcrInstallStatus";
import type { AutomationAction as AutomationActionGen } from "./types/generated/AutomationAction";
import type { AutomationRunSummary as AutomationRunSummaryGen } from "./types/generated/AutomationRunSummary";
import type { AutomationRunDetail as AutomationRunDetailGen } from "./types/generated/AutomationRunDetail";
import type { AutomationInput } from "./types/generated/AutomationInput";
import type { ClipboardCaptured } from "./types/generated/ClipboardCaptured";
import type { SettingsChanged } from "./types/generated/SettingsChanged";
import type { ClipUpdate } from "./types/generated/ClipUpdate";
import type { AppInfo } from "./types/generated/AppInfo";
import type { CloudSettings } from "./types/generated/CloudSettings";
import type { OcrInstallProgress } from "./types/generated/OcrInstallProgress";
import type { ImageOcrResult } from "./types/generated/ImageOcrResult";
import type { ImageOcrWord } from "./types/generated/ImageOcrWord";
import type { ScreenshotSelection } from "./types/generated/ScreenshotSelection";
import type { OcrResultPayload } from "./types/generated/OcrResultPayload";
import type { ListeningChanged } from "./types/generated/ListeningChanged";
import type { AppendCopyChanged } from "./types/generated/AppendCopyChanged";
import type { PanelVisibilityChanged } from "./types/generated/PanelVisibilityChanged";
import type { Category } from "./types/generated/Category";

// —— 窄化：Rust 字符串字段 → 前端字面量联合 ——

export type ClipItem = Omit<ClipItemGen, "clipType"> & { clipType: ClipType };

export type CategoryItem = Omit<CategoryItemGen, "clipType" | "syncState"> & {
  clipType: ClipType;
  syncState: SyncState;
};

export type AppSnapshot = Omit<AppSnapshotGen, "clips" | "categories" | "categoryItems" | "settings"> & {
  clips: ClipItem[];
  categories: Category[];
  categoryItems: CategoryItem[];
  settings: AppSettings;
};

export type ClipPage = Omit<ClipPageGen, "clips"> & { clips: ClipItem[] };

export type CategoryWithItem = Omit<CategoryWithItemGen, "item"> & { item: CategoryItem };

export type CategoryHitGroup = Omit<CategoryHitGroupGen, "items"> & { items: CategoryItem[] };

export type SearchResult =
  | (Omit<Extract<SearchResultGen, { kind: "history" }>, "page"> & { page: ClipPage })
  | (Omit<Extract<SearchResultGen, { kind: "categoryHits" }>, "groups"> & { groups: CategoryHitGroup[] });

export type OcrInstallStatus = Omit<OcrInstallStatusGen, "mode"> & { mode: OcrMode | "mocr" };

export type AutomationRunSummary = Omit<AutomationRunSummaryGen, "status"> & { status: AutomationStatus };

export type AutomationRunDetail = Omit<AutomationRunDetailGen, "status"> & { status: AutomationStatus };

export type AutomationAction = Omit<AutomationActionGen, "lastRun"> & { lastRun: AutomationRunSummary | null };

// —— 直接再导出（形状与前端现状一致）——

export type { AppInfo, AutomationInput, CloudSettings, ClipUpdate, OcrInstallProgress, ImageOcrResult, ImageOcrWord };
export type { ScreenshotSelection, OcrResultPayload };
export type { AppSettings, Language, OcrEngine, OcrMode, PanelLayout, PanelOpenBehavior };
export type { CloudOcrSettings } from "./types/generated/CloudOcrSettings";
export type { CloudOcrPromptMessage } from "./types/generated/CloudOcrPromptMessage";
export type { Category };

// —— 事件 payload（沿用旧名）——

export type CapturedEvent = Omit<ClipboardCaptured, "clip"> & { clip: ClipItem };
export type ListeningChangedEvent = ListeningChanged;
export type AppendCopyChangedEvent = AppendCopyChanged;
export type SettingsChangedEvent = Omit<SettingsChanged, "settings"> & { settings: AppSettings };
export type PanelVisibilityChangedEvent = PanelVisibilityChanged;

// —— 纯前端类型（Rust 侧无对应物）——

export type ClipViewItem =
  | (ClipItem & { collection: "history" })
  | (CategoryItem & { collection: "category" });

export type ClipViewerPayload = {
  label: string;
  originalClipId: string;
  item: ClipViewItem;
};

/** 由前端自己 emit（useClipEditor），Rust 不发起。 */
export type ClipUpdatedEvent = {
  collection: "history" | "category";
  item: ClipItem | CategoryItem;
  mergedFromId?: string;
};

// automation.rs 的三个事件 payload 由 serde_json::json! 内联构造，无 Rust 结构体，
// 暂留前端手写（第 2 阶段拆 ocr/automation 时再评估是否建结构体）。
export type AutomationRunStartedEvent = { runId: string; automationId: string; startedAt: string };
export type AutomationRunOutputEvent = { runId: string; automationId: string; stream: "stdout" | "stderr"; chunk: string };
export type AutomationRunFinishedEvent = { runId: string; automationId: string; status: AutomationStatus; exitCode?: number | null; startedAt: string; finishedAt: string };
