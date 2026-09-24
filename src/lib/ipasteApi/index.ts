import { clipsApi } from "./clips";
import { categoriesApi } from "./categories";
import { settingsApi } from "./settings";
import { ocrApi } from "./ocr";
import { deviceSyncApi } from "./deviceSync";
import { automationsApi } from "./automations";
import { call } from "./call";

export { call } from "./call";

export const ipasteApi = {
  ...clipsApi,
  ...categoriesApi,
  ...settingsApi,
  ...ocrApi,
  ...deviceSyncApi,
  ...automationsApi,
  setMainWindowDragging(dragging: boolean) {
    return call<void>("set_main_window_dragging", { dragging });
  },
  startMainWindowDrag() {
    return call<boolean>("start_main_window_drag", undefined, false);
  },
};

export { clipViewerStorageKey } from "./clips";
export type { LanClipSource } from "./deviceSync";
