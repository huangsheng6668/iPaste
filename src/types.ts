// 共享契约类型：结构字段来自 src/types/generated（ts-rs 从 Rust 生成，
// 运行 npm run gen:types 再生成）。本文件只做三件事：
// 1. 再导出生成类型（保持既有类型名不变）；
// 2. 用 Omit & 交集恢复前端的窄字面量联合（Rust 侧是 String）；
// 3. 定义纯前端类型（ClipViewItem、ClipUpdatedEvent 等）。
// 不允许在本文件手写与 Rust 重复的结构字段。

export type ClipType = "text" | "link" | "color" | "image" | "file" | "html";
export type PanelOpenBehavior = "history" | "last_selected";
export type PanelLayout = "top" | "side";
export type OcrMode = "fast" | "best";
export type Language = "en" | "zh-CN" | "ja" | "ko" | "es" | "fr" | "de";
export type AutomationStatus = "idle" | "running" | "success" | "failed" | "timed_out";
export type SyncState = "local" | "syncing" | "synced" | "conflict";

import type { ClipItem as ClipItemGen } from "./types/generated/ClipItem";
import type { CategoryItem as CategoryItemGen } from "./types/generated/CategoryItem";
import type { AppSnapshot as AppSnapshotGen } from "./types/generated/AppSnapshot";
import type { ClipPage as ClipPageGen } from "./types/generated/ClipPage";
import type { CategoryWithItem as CategoryWithItemGen } from "./types/generated/CategoryWithItem";
import type { CategoryHitGroup as CategoryHitGroupGen } from "./types/generated/CategoryHitGroup";
import type { SearchResult as SearchResultGen } from "./types/generated/SearchResult";
import type { AppSettings as AppSettingsGen } from "./types/generated/AppSettings";
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
import type { LanRole } from "./types/generated/LanRole";
import type { LanStatus } from "./types/generated/LanStatus";
import type { LanSessionInfo } from "./types/generated/LanSessionInfo";
import type { ClipSource } from "./types/generated/ClipSource";
import type { LanPairRequest } from "./types/generated/LanPairRequest";
import type { LanSessionReady } from "./types/generated/LanSessionReady";
import type { LanDisconnected } from "./types/generated/LanDisconnected";
import type { LanClipReceived } from "./types/generated/LanClipReceived";
import type { LanJoinFailed } from "./types/generated/LanJoinFailed";
import type { LanClipReceiveFailed } from "./types/generated/LanClipReceiveFailed";
import type { LanGuestRejected } from "./types/generated/LanGuestRejected";
import type { LanCategorySent } from "./types/generated/LanCategorySent";
import type { LanCategoryReceived } from "./types/generated/LanCategoryReceived";
import type { PortConflict } from "./types/generated/PortConflict";
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

export type AppSettings = Omit<AppSettingsGen, "panelOpenBehavior" | "panelLayout" | "ocrMode" | "language"> & {
  panelOpenBehavior: PanelOpenBehavior;
  panelLayout: PanelLayout;
  ocrMode: OcrMode;
  language: Language;
};

export type OcrInstallStatus = Omit<OcrInstallStatusGen, "mode"> & { mode: OcrMode };

export type AutomationRunSummary = Omit<AutomationRunSummaryGen, "status"> & { status: AutomationStatus };

export type AutomationRunDetail = Omit<AutomationRunDetailGen, "status"> & { status: AutomationStatus };

export type AutomationAction = Omit<AutomationActionGen, "lastRun"> & { lastRun: AutomationRunSummary | null };

// —— 直接再导出（形状与前端现状一致）——

export type { AppInfo, AutomationInput, CloudSettings, ClipUpdate, OcrInstallProgress, ImageOcrResult, ImageOcrWord };
export type { LanRole, LanStatus, LanSessionInfo, PortConflict, Category };
export type { LanPairRequest, LanSessionReady, LanDisconnected, LanClipReceived, LanJoinFailed };
export type { LanClipReceiveFailed, LanGuestRejected, LanCategorySent, LanCategoryReceived };

// —— 事件 payload（沿用旧名）——

export type CapturedEvent = Omit<ClipboardCaptured, "clip"> & { clip: ClipItem };
export type ListeningChangedEvent = ListeningChanged;
export type AppendCopyChangedEvent = AppendCopyChanged;
export type SettingsChangedEvent = Omit<SettingsChanged, "settings"> & { settings: AppSettings };
export type PanelVisibilityChangedEvent = PanelVisibilityChanged;

// —— LAN 事件 payload（沿用旧名）——

export type LanClipReceivedEvent = LanClipReceived;
export type LanCategorySentEvent = LanCategorySent;
export type LanCategoryReceivedEvent = LanCategoryReceived;
export type LanPairRequestEvent = LanPairRequest;

export type LanClipSource = ClipSource;

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
