<script setup lang="ts">
import { Clipboard, Info, Link } from "lucide-vue-next";
import { computed } from "vue";
import AutomationDetailPane from "./AutomationDetailPane.vue";
import { clipImageSrc } from "../lib/clipMedia";
import { t } from "../i18n";
import { clipMetricText, formatTime, typeLabel } from "../lib/format";
import type { AutomationAction, ClipViewItem } from "../types";

const props = defineProps<{
  item?: ClipViewItem;
  mode?: "clip" | "actions";
  automationAction?: AutomationAction | null;
}>();

const emit = defineEmits<{ (e: "run-automation"): void }>();

const lines = computed(() => props.item?.text.split(/\r?\n/).length ?? 0);
const isImage = computed(() => props.item?.clipType === "image");
const imageSrc = computed(() => props.item ? clipImageSrc(props.item) : "");
const detailTitle = computed(() => {
  if (!props.item) return "";
  return props.item.displayName?.trim() || t("clip.clipboardTitle", { type: typeLabel(props.item.clipType) });
});
const displayTime = computed(() => {
  if (!props.item) return "";
  return props.item.collection === "history" ? props.item.lastCapturedAt : props.item.createdAt;
});
</script>

<template>
  <aside class="detail-pane hidden w-60 shrink-0 border-l border-slate-200 lg:block">
    <AutomationDetailPane
      v-if="mode === 'actions'"
      :action="automationAction ?? null"
      @run="emit('run-automation')"
    />
    <div
      v-else-if="item"
      class="flex h-full flex-col"
    >
      <div class="border-b border-slate-200 p-4">
        <div class="detail-pane-kicker flex items-center gap-2">
          <Info class="size-3.5" />
          {{ t("detail.title") }}
        </div>
        <h2 class="detail-pane-title mt-2 truncate text-base font-semibold">
          {{ detailTitle }}
        </h2>
        <p class="detail-pane-time mt-1 text-xs">
          {{ formatTime(displayTime) }}
        </p>
      </div>

      <div class="flex-1 overflow-y-auto p-4">
        <div
          v-if="item.clipType === 'color'"
          class="mb-4 h-24 rounded-lg border border-slate-200"
          :style="{ backgroundColor: item.text.trim() }"
        />

        <a
          v-if="item.clipType === 'link'"
          class="mb-4 flex items-center gap-2 rounded-lg border border-slate-200 px-3 py-2 text-sm text-teal-700 transition hover:bg-teal-50"
          :href="item.text"
          target="_blank"
          tabindex="-1"
        >
          <Link class="size-4" />
          <span class="truncate">{{ item.text }}</span>
        </a>

        <div
          v-if="isImage"
          class="mb-4 overflow-hidden rounded-xl border border-slate-200 bg-slate-50"
        >
          <img
            class="max-h-[360px] w-full object-contain"
            :src="imageSrc"
            :alt="t('common.imagePreviewAlt')"
          >
        </div>

        <pre
          v-if="!isImage"
          class="detail-pane-code max-h-[320px] overflow-auto whitespace-pre-wrap break-words rounded-lg p-3 text-sm leading-5"
        >{{ item.text }}</pre>

        <dl class="mt-4 grid grid-cols-2 gap-3 text-sm">
          <div>
            <dt class="detail-pane-label text-xs">
              {{ t("common.size") }}
            </dt>
            <dd class="detail-pane-value mt-1">
              {{ clipMetricText(item.clipType, item.text, item.previewText) }}
            </dd>
          </div>
          <div v-if="!isImage">
            <dt class="detail-pane-label text-xs">
              {{ t("common.lines") }}
            </dt>
            <dd class="detail-pane-value mt-1">
              {{ lines }}
            </dd>
          </div>
        </dl>
      </div>
    </div>

    <div
      v-else
      class="detail-pane-label flex h-full flex-col items-center justify-center gap-3 px-8 text-center"
    >
      <Clipboard class="size-8" />
      <p class="text-sm">
        {{ t("detail.noSelection") }}
      </p>
    </div>
  </aside>
</template>
