export type ClipType = "text" | "link" | "color" | "image" | "file" | "html";

export type ClipItem = {
  id: string;
  clipType: ClipType;
  contentHash: string;
  displayName?: string | null;
  previewText: string;
  text: string;
  sourceApp?: string | null;
  lastCapturedAt: string;
  favoriteCount: number;
  isPinned: boolean;
};

export type Category = {
  id: string;
  name: string;
  color: string;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
};

export type CategoryItem = {
  id: string;
  categoryId: string;
  clipSnapshotId: string;
  clipType: ClipType;
  contentHash: string;
  displayName?: string | null;
  previewText: string;
  text: string;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
  syncState: "local" | "syncing" | "synced" | "conflict";
  isPinned: boolean;
};

export type CategoryWithItem = {
  category: Category;
  item: CategoryItem;
};

export type PanelOpenBehavior = "history" | "last_selected";
export type PanelLayout = "top" | "side";
export type OcrMode = "fast" | "best";
export type Language = "en" | "zh-CN" | "ja" | "ko" | "es" | "fr" | "de";

export type AppSnapshot = {
  clips: ClipItem[];
  hasMoreClips: boolean;
  clipTotalCount: number;
  categories: Category[];
  categoryItems: CategoryItem[];
  shortcut: string;
  isListening: boolean;
  isAppendCopyEnabled: boolean;
  settings: AppSettings;
};

export type ClipPage = {
  clips: ClipItem[];
  hasMore: boolean;
  totalCount: number;
  allCount: number;
};

export type AppSettings = {
  shortcut: string;
  retentionDays: number;
  appendCopyTimeoutMinutes: number;
  panelOpenBehavior: PanelOpenBehavior;
  panelLayout: PanelLayout;
  ocrMode: OcrMode;
  language: Language;
  cloud: CloudSettings;
};

export type AppInfo = {
  version: string;
};

export type OcrInstallStatus = {
  installed: boolean;
  engineId: string;
  engineVersion?: string | null;
  mode: OcrMode;
  platform: string;
  manifestUrl: string;
  installDir: string;
  downloadedBytes: number;
  totalBytes: number;
  missingFiles: string[];
};

export type OcrInstallProgress = {
  phase: "fetchingManifest" | "downloading" | "completed" | string;
  fileName?: string | null;
  downloadedBytes: number;
  totalBytes: number;
};

export type ImageOcrResult = {
  text: string;
  engine: string;
  language: string;
  words: ImageOcrWord[];
};

export type ImageOcrWord = {
  text: string;
  left: number;
  top: number;
  width: number;
  height: number;
  confidence: number;
  blockIndex?: number;
  paragraphIndex?: number;
  lineIndex?: number;
  wordIndex?: number;
};

export type CloudSettings = {
  apiAddress: string;
  apiKey: string;
  enabled: boolean;
  lastConnectedAt?: string | null;
};

export type CapturedEvent = {
  clip: ClipItem;
  clipTotalCount: number;
  wasInserted: boolean;
};

export type ListeningChangedEvent = {
  isListening: boolean;
};

export type AppendCopyChangedEvent = {
  isEnabled: boolean;
};

export type SettingsChangedEvent = {
  settings: AppSettings;
};

export type ClipViewItem =
  | (ClipItem & { collection: "history" })
  | (CategoryItem & { collection: "category" });

export type ClipViewerPayload = {
  label: string;
  originalClipId: string;
  item: ClipViewItem;
};

export type ClipUpdatedEvent = {
  collection: "history" | "category";
  item: ClipItem | CategoryItem;
  mergedFromId?: string;
};

export type CategoryHitGroup = {
  category: Category;
  items: CategoryItem[];
};

export type SearchResult =
  | { kind: "history"; page: ClipPage }
  | { kind: "categoryHits"; groups: CategoryHitGroup[] };

export type AutomationStatus = "idle" | "running" | "success" | "failed" | "timed_out";

export type AutomationAction = {
  id: string;
  name: string;
  command: string;
  cwd?: string | null;
  runMode: string;
  confirmBeforeRun: boolean;
  closePanelOnSuccess: boolean;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
  lastRun?: AutomationRunSummary | null;
};

export type AutomationRunSummary = {
  id: string;
  status: AutomationStatus;
  exitCode?: number | null;
  startedAt: string;
  finishedAt?: string | null;
  durationMs?: number | null;
};

export type AutomationRunDetail = {
  id: string;
  automationId: string;
  status: AutomationStatus;
  exitCode?: number | null;
  stdout: string;
  stderr: string;
  stdoutTruncated: boolean;
  stderrTruncated: boolean;
  startedAt: string;
  finishedAt?: string | null;
  durationMs?: number | null;
};

export type AutomationInput = {
  name: string;
  command: string;
  cwd?: string | null;
  confirmBeforeRun: boolean;
  closePanelOnSuccess: boolean;
};

export type AutomationRunStartedEvent = { runId: string; automationId: string; startedAt: string };
export type AutomationRunOutputEvent = { runId: string; automationId: string; stream: "stdout" | "stderr"; chunk: string };
export type AutomationRunFinishedEvent = { runId: string; automationId: string; status: AutomationStatus; exitCode?: number | null; startedAt: string; finishedAt: string };

export type LanRole = "host" | "guest";
export type LanStatus = "idle" | "hosting" | "waitingPair" | "connected";
export interface LanSessionInfo {
  role: LanRole | null;
  status: LanStatus;
  code: string | null;
  listenAddr: string | null;
  peerDeviceName: string | null;
}
export type LanClipSource = { kind: "current" } | { kind: "item"; id: string };

export interface LanDevice {
  deviceName: string;
  addr: string;
}

export interface PortConflict {
  pid: number;
  name: string;
}
