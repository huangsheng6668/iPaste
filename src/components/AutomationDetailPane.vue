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
    class="flex h-full flex-col"
  >
    <div class="flex items-center justify-between border-b border-slate-200 px-4 py-3">
      <div class="min-w-0">
        <h2 class="truncate text-sm font-semibold text-slate-950">
          {{ action.name }}
        </h2>
        <span
          class="text-xs"
          :class="status === 'running' ? 'text-blue-600' : status === 'success' ? 'text-emerald-600' : status === 'failed' || status === 'timed_out' ? 'text-red-600' : 'text-slate-400'"
        >{{ statusLabel }}</span>
      </div>
      <button
        type="button"
        class="flex items-center gap-1 rounded-lg bg-slate-900 px-3 py-1.5 text-sm text-white hover:bg-slate-700 disabled:opacity-50"
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

    <div class="flex-1 overflow-auto px-4 py-3 text-sm">
      <dl class="space-y-2">
        <div>
          <dt class="text-xs font-medium text-slate-500">
            {{ t("automation.command") }}
          </dt>
          <dd class="mt-0.5 break-all rounded bg-slate-50 p-2 font-mono text-xs">
            {{ action.command }}
          </dd>
        </div>
        <div v-if="action.cwd">
          <dt class="text-xs font-medium text-slate-500">
            {{ t("automation.cwd") }}
          </dt>
          <dd class="mt-0.5 font-mono text-xs text-slate-700">
            {{ action.cwd }}
          </dd>
        </div>
        <div
          v-if="action.lastRun"
          class="flex gap-4"
        >
          <div>
            <dt class="text-xs font-medium text-slate-500">
              {{ t("automation.detailExitCode") }}
            </dt>
            <dd class="text-xs">
              {{ action.lastRun.exitCode ?? "—" }}
            </dd>
          </div>
          <div>
            <dt class="text-xs font-medium text-slate-500">
              {{ t("automation.detailDuration") }}
            </dt>
            <dd class="text-xs">
              {{ action.lastRun.durationMs != null ? `${action.lastRun.durationMs} ms` : "—" }}
            </dd>
          </div>
        </div>
      </dl>

      <div
        v-if="loading"
        class="mt-3 text-xs text-slate-400"
      >
        {{ t("automation.statusRunning") }}…
      </div>
      <template v-else>
        <div
          v-if="runningLogs.stdout"
          class="mt-3"
        >
          <dt class="text-xs font-medium text-slate-500">
            {{ t("automation.detailStdout") }}
          </dt>
          <pre class="mt-0.5 max-h-48 overflow-auto rounded bg-slate-50 p-2 font-mono text-xs">{{ runningLogs.stdout }}</pre>
          <p
            v-if="detail?.stdoutTruncated"
            class="mt-0.5 text-xs text-slate-400"
          >
            {{ t("automation.logTruncated") }}
          </p>
        </div>
        <div
          v-if="runningLogs.stderr"
          class="mt-2"
        >
          <dt class="text-xs font-medium text-slate-500">
            {{ t("automation.detailStderr") }}
          </dt>
          <pre class="mt-0.5 max-h-48 overflow-auto rounded bg-red-50 p-2 font-mono text-xs text-red-700">{{ runningLogs.stderr }}</pre>
          <p
            v-if="detail?.stderrTruncated"
            class="mt-0.5 text-xs text-slate-400"
          >
            {{ t("automation.logTruncated") }}
          </p>
        </div>
      </template>
    </div>
  </div>
</template>
