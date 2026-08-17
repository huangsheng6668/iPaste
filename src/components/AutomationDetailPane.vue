<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { Play, LoaderCircle } from "lucide-vue-next";
import { t } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import type { AutomationAction, AutomationRunDetail } from "../types";

const props = defineProps<{ action: AutomationAction | null }>();
const emit = defineEmits<{ (e: "run"): void }>();

const detail = ref<AutomationRunDetail | null>(null);
const loading = ref(false);

const status = computed(() => props.action?.lastRun?.status ?? "idle");
const statusLabel = computed(() => {
  const map: Record<string, string> = {
    idle: t("automation.statusIdle"),
    running: t("automation.statusRunning"),
    success: t("automation.statusSuccess"),
    failed: t("automation.statusFailed"),
    timed_out: t("automation.statusTimedOut"),
  };
  return map[status.value] ?? "";
});
const runningLogs = computed(() => {
  const runId = props.action?.lastRun?.id;
  if (!runId) return { stdout: "", stderr: "" };
  return props.action?.lastRun?.status === "running" && detail.value
    ? { stdout: detail.value.stdout, stderr: detail.value.stderr }
    : { stdout: detail.value?.stdout ?? "", stderr: detail.value?.stderr ?? "" };
});

async function loadDetail() {
  detail.value = null;
  const runId = props.action?.lastRun?.id;
  if (!runId) return;
  loading.value = true;
  try {
    detail.value = await ipasteApi.getAutomationRun(runId);
  } catch {
    /* ignore */
  }
  loading.value = false;
}

watch(
  () => [props.action?.id, props.action?.lastRun?.status],
  () => {
    if (props.action?.lastRun && props.action.lastRun.status !== "running") {
      void loadDetail();
    } else if (props.action?.lastRun?.id) {
      void loadDetail();
    }
  },
  { immediate: true },
);
</script>

<template>
  <div
    v-if="action"
    class="flex h-full flex-col bg-[var(--surface)] text-[var(--text-1)]"
  >
    <div class="flex items-center justify-between border-b border-[var(--border-hairline)] px-4 py-3 bg-[var(--surface-2)]">
      <div class="min-w-0">
        <h2 class="truncate text-sm font-semibold text-[var(--text-1)]">
          {{ action.name }}
        </h2>
        <span
          class="text-xs"
          :class="status === 'running' ? 'text-[var(--info)]' : status === 'success' ? 'text-[var(--success)]' : status === 'failed' || status === 'timed_out' ? 'text-[var(--danger)]' : 'text-[var(--text-3)]'"
        >{{ statusLabel }}</span>
      </div>
      <button
        type="button"
        class="btn-primary text-xs"
        :disabled="status === 'running'"
        @click="emit('run')"
      >
        <LoaderCircle
          v-if="status === 'running'"
          class="size-3.5 update-spin"
        />
        <Play
          v-else
          class="size-3.5"
        />
        <span>{{ t("automation.run") }}</span>
      </button>
    </div>

    <div class="flex-1 overflow-auto p-4 text-sm">
      <dl class="space-y-3">
        <div>
          <dt class="text-xs font-medium text-[var(--text-3)]">
            {{ t("automation.command") }}
          </dt>
          <dd class="mt-1 break-all rounded-md border border-[var(--border-hairline)] bg-[var(--surface-code)] p-2.5 font-mono text-xs text-[var(--text-1)]">
            {{ action.command }}
          </dd>
        </div>
        <div v-if="action.cwd">
          <dt class="text-xs font-medium text-[var(--text-3)]">
            {{ t("automation.cwd") }}
          </dt>
          <dd class="mt-1 font-mono text-xs text-[var(--text-2)]">
            {{ action.cwd }}
          </dd>
        </div>
        <div
          v-if="action.lastRun"
          class="flex gap-4"
        >
          <div>
            <dt class="text-xs font-medium text-[var(--text-3)]">
              {{ t("automation.detailExitCode") }}
            </dt>
            <dd class="text-xs font-mono text-[var(--text-2)]">
              {{ action.lastRun.exitCode ?? "—" }}
            </dd>
          </div>
          <div>
            <dt class="text-xs font-medium text-[var(--text-3)]">
              {{ t("automation.detailDuration") }}
            </dt>
            <dd class="text-xs font-mono text-[var(--text-2)]">
              {{ action.lastRun.durationMs != null ? `${action.lastRun.durationMs} ms` : "—" }}
            </dd>
          </div>
        </div>
      </dl>

      <div
        v-if="loading"
        class="mt-3 text-xs text-[var(--text-3)]"
      >
        {{ t("automation.statusRunning") }}…
      </div>
      <template v-else>
        <div
          v-if="runningLogs.stdout"
          class="mt-3"
        >
          <dt class="text-xs font-medium text-[var(--text-3)]">
            {{ t("automation.detailStdout") }}
          </dt>
          <pre class="mt-1 max-h-48 overflow-auto rounded-md border border-[var(--border-hairline)] bg-[var(--surface-code)] p-2.5 font-mono text-xs text-[var(--text-1)]">{{ runningLogs.stdout }}</pre>
          <p
            v-if="detail?.stdoutTruncated"
            class="mt-0.5 text-xs text-[var(--text-3)]"
          >
            {{ t("automation.logTruncated") }}
          </p>
        </div>
        <div
          v-if="runningLogs.stderr"
          class="mt-2"
        >
          <dt class="text-xs font-medium text-[var(--text-3)]">
            {{ t("automation.detailStderr") }}
          </dt>
          <pre class="mt-1 max-h-48 overflow-auto rounded-md border border-[var(--danger-border)] bg-[var(--danger-soft)] p-2.5 font-mono text-xs text-[var(--danger)]">{{ runningLogs.stderr }}</pre>
          <p
            v-if="detail?.stderrTruncated"
            class="mt-0.5 text-xs text-[var(--text-3)]"
          >
            {{ t("automation.logTruncated") }}
          </p>
        </div>
      </template>
    </div>
  </div>
</template>
