import { describe, expect, it } from "vitest";
import {
  DEFAULT_LANGUAGE,
  DEFAULT_OCR_ENGINE,
  DEFAULT_OCR_MODE,
  DEFAULT_PANEL_LAYOUT,
  SETTINGS_SCHEMA,
  SETTING_KEYS,
  cleanAppendCopyTimeoutMinutes,
  cleanOcrEngine,
  cleanOcrMode,
  cleanPanelLayout,
  type SettingKey,
} from "./settings";

// 表驱动：设置项登记表的自洽性（Task 37）。
// 目的不是测清洗器本身（stores/lib/settings.test.ts 已覆盖），
// 而是保证"登记表 → 清洗器 → 默认值 → 命令名"这层映射不会写错。

/** 每个键的合法默认值与一个非法值（非法值必须被清洗器归一化或回落默认）。 */
const CASES: Record<SettingKey, { valid: unknown; invalid: unknown }> = {
  shortcut: { valid: "CommandOrControl+Shift+V", invalid: "不是快捷键" },
  ocrShortcut: { valid: "CommandOrControl+Shift+O", invalid: "不是快捷键" },
  panelOpenBehavior: { valid: "history", invalid: "bogus" },
  appendCopyTimeoutMinutes: { valid: 3, invalid: 999 },
  panelLayout: { valid: "side", invalid: "diagonal" },
  ocrMode: { valid: "best", invalid: "turbo" },
  ocrEngine: { valid: "openai", invalid: "bigmodel" },
  language: { valid: "zh-CN", invalid: "klingon" },
};

describe("SETTINGS_SCHEMA 登记表", () => {
  it("键集合与 SETTING_KEYS 一致且非空", () => {
    expect(SETTING_KEYS).toEqual(Object.keys(SETTINGS_SCHEMA));
    expect(SETTING_KEYS.length).toBeGreaterThan(0);
  });

  it("每个键都登记了非空的后端命令名，且命令名互不重复", () => {
    const commands = SETTING_KEYS.map((key) => SETTINGS_SCHEMA[key].command);
    for (const command of commands) expect(command.length).toBeGreaterThan(0);
    expect(new Set(commands).size).toBe(commands.length);
  });

  it("乐观写的键限定在本地有清洗器的那几项（回显式键不做本地清洗）", () => {
    for (const key of SETTING_KEYS) {
      const spec = SETTINGS_SCHEMA[key];
      // 回显式（optimistic=false）的键，其 clean 必须是恒等映射：
      // 这些值由后端校验并整体回填，前端不做本地改写。
      if (!spec.optimistic) {
        expect(spec.clean(CASES[key].valid as never)).toBe(CASES[key].valid);
      }
    }
  });

  it("每个键的清洗器：合法值原样通过（或归一化后仍合法）", () => {
    expect(SETTINGS_SCHEMA.appendCopyTimeoutMinutes.clean(3)).toBe(3);
    expect(SETTINGS_SCHEMA.panelLayout.clean("side")).toBe("side");
    expect(SETTINGS_SCHEMA.ocrMode.clean("best")).toBe("best");
    expect(SETTINGS_SCHEMA.ocrEngine.clean("openai")).toBe("openai");
    expect(SETTINGS_SCHEMA.language.clean("zh-CN")).toBe("zh-CN");
  });

  it("每个键的清洗器：非法值不得原样通过", () => {
    expect(cleanAppendCopyTimeoutMinutes(CASES.appendCopyTimeoutMinutes.invalid)).toBe(1);
    expect(cleanPanelLayout(CASES.panelLayout.invalid)).toBe(DEFAULT_PANEL_LAYOUT);
    expect(cleanOcrMode(CASES.ocrMode.invalid)).toBe(DEFAULT_OCR_MODE);
    expect(cleanOcrEngine(CASES.ocrEngine.invalid)).toBe(DEFAULT_OCR_ENGINE);
    expect(SETTINGS_SCHEMA.language.clean(CASES.language.invalid)).toBe(DEFAULT_LANGUAGE);
  });

  it("清洗器幂等：清洗结果再清洗一次不变（乐观写会连续赋值）", () => {
    const chain = [
      SETTINGS_SCHEMA.appendCopyTimeoutMinutes.clean(3),
      SETTINGS_SCHEMA.panelLayout.clean("side"),
      SETTINGS_SCHEMA.ocrMode.clean("best"),
      SETTINGS_SCHEMA.ocrEngine.clean("openai"),
      SETTINGS_SCHEMA.language.clean("zh-CN"),
    ];
    const again = [
      SETTINGS_SCHEMA.appendCopyTimeoutMinutes.clean(chain[0] as never),
      SETTINGS_SCHEMA.panelLayout.clean(chain[1] as never),
      SETTINGS_SCHEMA.ocrMode.clean(chain[2] as never),
      SETTINGS_SCHEMA.ocrEngine.clean(chain[3] as never),
      SETTINGS_SCHEMA.language.clean(chain[4] as never),
    ];
    expect(again).toEqual(chain);
  });
});
