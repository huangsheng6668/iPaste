import type { AutomationAction, AutomationInput } from "../../types";

export type AutomationExport = {
  version: number;
  exportedAt: string;
  automations: Array<{
    name: string;
    command: string;
    cwd: string | null;
    confirmBeforeRun: boolean;
    closePanelOnSuccess: boolean;
  }>;
};

export type ImportResult = {
  valid: AutomationInput[];
  skippedDuplicates: number;
  skippedInvalid: number;
};

const COMMAND_MAX_LEN = 4000;

export function serializeAutomations(actions: AutomationAction[]): string {
  const payload: AutomationExport = {
    version: 1,
    exportedAt: new Date().toISOString(),
    automations: actions.map((a) => ({
      name: a.name,
      command: a.command,
      cwd: a.cwd ?? null,
      confirmBeforeRun: a.confirmBeforeRun,
      closePanelOnSuccess: a.closePanelOnSuccess,
    })),
  };
  return JSON.stringify(payload, null, 2);
}

export function parseImportFile(text: string, existingNames: Set<string>): ImportResult {
  let data: unknown;
  try {
    data = JSON.parse(text);
  } catch {
    throw new Error("invalid-json");
  }

  const raw = (data as { automations?: unknown }).automations;
  if (!Array.isArray(raw)) {
    throw new Error("no-automations-array");
  }

  const valid: AutomationInput[] = [];
  let skippedDuplicates = 0;
  let skippedInvalid = 0;
  const seenNames = new Set(existingNames);

  for (const entry of raw) {
    if (!entry || typeof entry !== "object") {
      skippedInvalid++;
      continue;
    }
    const e = entry as Record<string, unknown>;
    const name = typeof e.name === "string" ? e.name.trim() : "";
    const command = typeof e.command === "string" ? e.command.trim() : "";

    if (!name || !command || [...command].length > COMMAND_MAX_LEN) {
      skippedInvalid++;
      continue;
    }

    if (seenNames.has(name)) {
      skippedDuplicates++;
      continue;
    }
    seenNames.add(name);

    valid.push({
      name,
      command,
      cwd: typeof e.cwd === "string" && e.cwd.trim() ? e.cwd : null,
      confirmBeforeRun: typeof e.confirmBeforeRun === "boolean" ? e.confirmBeforeRun : false,
      closePanelOnSuccess: typeof e.closePanelOnSuccess === "boolean" ? e.closePanelOnSuccess : false,
    });
  }

  return { valid, skippedDuplicates, skippedInvalid };
}
