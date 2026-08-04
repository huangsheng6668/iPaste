import type { AutomationAction } from "../../types";

export function automationMatchesQuery(action: AutomationAction, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return (
    action.name.toLowerCase().includes(q) ||
    action.command.toLowerCase().includes(q) ||
    (action.cwd ?? "").toLowerCase().includes(q)
  );
}

export function filterAutomations(actions: AutomationAction[], query: string): AutomationAction[] {
  return actions.filter((action) => automationMatchesQuery(action, query));
}
