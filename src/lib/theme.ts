import { computed, ref } from "vue";

/**
 * 外观主题（浅色 / 深色 / 跟随系统）。
 *
 * 偏好持久化在 localStorage（与 `ipaste.language` 一致），在 main.ts
 * 挂载前调用 initTheme() 应用，避免首屏闪烁。解析结果通过
 * `<html class="dark">` 与 `color-scheme` 暴露给样式层。
 */
export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

const STORAGE_KEY = "ipaste.theme";
const DARK_CLASS = "dark";

function readStoredPreference(): ThemePreference {
  try {
    const value = localStorage.getItem(STORAGE_KEY);
    if (value === "light" || value === "dark") return value;
  } catch {
    /* localStorage 不可用时回退到 system */
  }
  return "system";
}

export const themePreference = ref<ThemePreference>(readStoredPreference());

const systemMedia =
  typeof window !== "undefined" && typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-color-scheme: dark)")
    : null;

export const resolvedTheme = computed<ResolvedTheme>(() => {
  if (themePreference.value === "system") {
    return systemMedia?.matches ? "dark" : "light";
  }
  return themePreference.value;
});

export function setThemePreference(preference: ThemePreference) {
  themePreference.value = preference;
  try {
    localStorage.setItem(STORAGE_KEY, preference);
  } catch {
    /* ignore */
  }
  applyTheme();
}

/** 在应用挂载前应用当前主题（main.ts 调用）。 */
export function initTheme() {
  applyTheme();
  systemMedia?.addEventListener("change", handleSystemChange);
  // 其他窗口（如设置窗口）修改主题时同步刷新本窗口。
  window.addEventListener("storage", handleStorageChange);
}

function handleSystemChange() {
  if (themePreference.value === "system") {
    applyTheme();
  }
}

function handleStorageChange(event: StorageEvent) {
  if (event.key !== STORAGE_KEY) return;
  const value = event.newValue;
  themePreference.value = value === "light" || value === "dark" ? value : "system";
  applyTheme();
}

function applyTheme() {
  const isDark = resolvedTheme.value === "dark";
  document.documentElement.classList.toggle(DARK_CLASS, isDark);
  document.documentElement.style.colorScheme = isDark ? "dark" : "light";
}
