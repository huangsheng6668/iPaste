<script setup lang="ts">
import { computed, ref } from "vue";
import {
  Check,
  Clipboard,
  ClipboardCopy,
  Code,
  CornerDownLeft,
  ExternalLink,
  Eye,
  FileText,
  Globe,
  Image as ImageIcon,
  Maximize2,
  Palette,
  ScanText,
  Type,
} from "lucide-vue-next";
import AutomationDetailPane from "./AutomationDetailPane.vue";
import { clipImageSrc } from "../lib/clipMedia";
import { t } from "../i18n";
import { clipMetricText, formatTime, lineCountText, textStats, typeLabel } from "../lib/format";
import type { AutomationAction, ClipViewItem } from "../types";

const props = defineProps<{
  item?: ClipViewItem | null;
  automationAction?: AutomationAction | null;
  mode?: "clip" | "actions";
}>();

const emit = defineEmits<{
  copy: [item: ClipViewItem];
  apply: [item: ClipViewItem];
  expand: [item: ClipViewItem];
  ocr: [item: ClipViewItem];
  runAutomation: [action: AutomationAction];
}>();

const copiedFormat = ref<string | null>(null);

const isImage = computed(() => props.item?.clipType === "image");
const isColor = computed(() => props.item?.clipType === "color");
const isLink = computed(() => props.item?.clipType === "link");
const isCode = computed(() => {
  if (props.item?.clipType === "html") return true;
  const text = props.item?.text || "";
  return /^(const|let|var|function|import|export|class|def|public|private|fn|impl|struct|enum|\{|<|SELECT|INSERT|UPDATE|DELETE)/m.test(text);
});

const imageSrc = computed(() => (props.item ? clipImageSrc(props.item) : ""));
const colorValue = computed(() => (props.item ? props.item.text.trim() : ""));
const lines = computed(() => props.item?.text.split(/\r?\n/).length ?? 0);

const colorFormats = computed(() => {
  if (!isColor.value || !colorValue.value) return [];
  const str = colorValue.value;
  const hexMatch = str.match(/^#?([0-9a-f]{3,8})$/i);
  let r = 0, g = 0, b = 0, a = 1;
  let parsed = false;

  if (hexMatch) {
    const hex = hexMatch[1];
    if (hex.length === 3) {
      r = parseInt(hex[0] + hex[0], 16);
      g = parseInt(hex[1] + hex[1], 16);
      b = parseInt(hex[2] + hex[2], 16);
      parsed = true;
    } else if (hex.length === 6) {
      r = parseInt(hex.slice(0, 2), 16);
      g = parseInt(hex.slice(2, 4), 16);
      b = parseInt(hex.slice(4, 6), 16);
      parsed = true;
    } else if (hex.length === 8) {
      r = parseInt(hex.slice(0, 2), 16);
      g = parseInt(hex.slice(2, 4), 16);
      b = parseInt(hex.slice(4, 6), 16);
      a = Math.round((parseInt(hex.slice(6, 8), 16) / 255) * 100) / 100;
      parsed = true;
    }
  }

  if (!parsed) {
    const rgbMatch = str.match(/rgba?\s*\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)(?:\s*,\s*([\d.]+))?\s*\)/i);
    if (rgbMatch) {
      r = Math.min(255, parseInt(rgbMatch[1], 10));
      g = Math.min(255, parseInt(rgbMatch[2], 10));
      b = Math.min(255, parseInt(rgbMatch[3], 10));
      if (rgbMatch[4] !== undefined) a = parseFloat(rgbMatch[4]);
      parsed = true;
    }
  }

  if (!parsed) {
    return [
      { label: "RAW", value: str },
      { label: "CSS", value: `color: ${str};` },
    ];
  }

  const toHex = (n: number) => n.toString(16).padStart(2, "0").toUpperCase();
  const hexVal = `#${toHex(r)}${toHex(g)}${toHex(b)}`;
  const rgbVal = a === 1 ? `rgb(${r}, ${g}, ${b})` : `rgba(${r}, ${g}, ${b}, ${a})`;

  const rNorm = r / 255, gNorm = g / 255, bNorm = b / 255;
  const max = Math.max(rNorm, gNorm, bNorm), min = Math.min(rNorm, gNorm, bNorm);
  let h = 0, s = 0, l = (max + min) / 2;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    switch (max) {
      case rNorm: h = (gNorm - bNorm) / d + (gNorm < bNorm ? 6 : 0); break;
      case gNorm: h = (bNorm - rNorm) / d + 2; break;
      case bNorm: h = (rNorm - gNorm) / d + 4; break;
    }
    h = Math.round(h * 60);
  }
  s = Math.round(s * 100);
  l = Math.round(l * 100);
  const hslVal = a === 1 ? `hsl(${h}, ${s}%, ${l}%)` : `hsla(${h}, ${s}%, ${l}%, ${a})`;

  return [
    { label: "HEX", value: hexVal },
    { label: "RGB", value: rgbVal },
    { label: "HSL", value: hslVal },
    { label: "CSS", value: `color: ${hexVal};` },
  ];
});

async function copyFormatValue(val: string, label: string) {
  try {
    await navigator.clipboard.writeText(val);
    copiedFormat.value = label;
    setTimeout(() => {
      if (copiedFormat.value === label) {
        copiedFormat.value = null;
      }
    }, 1500);
  } catch (_err) {
    void _err;
  }
}

const detailTitle = computed(() => {
  if (!props.item) return "";
  return props.item.displayName?.trim() || typeLabel(props.item.clipType);
});

const displayTime = computed(() => {
  if (!props.item) return "";
  return props.item.collection === "history" ? props.item.lastCapturedAt : props.item.createdAt;
});

const typeIcon = computed(() => {
  if (!props.item) return Clipboard;
  if (props.item.clipType === "image") return ImageIcon;
  if (props.item.clipType === "color") return Palette;
  if (props.item.clipType === "link") return ExternalLink;
  if (props.item.clipType === "file") return FileText;
  if (isCode.value) return Code;
  return Type;
});

const linkHostname = computed(() => {
  if (!isLink.value || !props.item?.text) return "";
  try {
    const url = new URL(props.item.text.trim());
    return url.hostname;
  } catch {
    return props.item.text.trim();
  }
});
</script>

<template>
  <aside class="clip-inspector">
    <!-- Automation Detail View -->
    <template v-if="mode === 'actions'">
      <AutomationDetailPane
        :action="automationAction ?? null"
        @run="automationAction && emit('runAutomation', automationAction)"
      />
    </template>

    <!-- Clip Item Inspector View -->
    <template v-else-if="item">
      <!-- Inspector Header -->
      <div class="clip-inspector-header">
        <div class="clip-inspector-header-left">
          <component
            :is="typeIcon"
            class="size-4 text-[var(--accent)]"
          />
          <span class="clip-inspector-title">{{ detailTitle }}</span>
        </div>

        <div class="flex items-center gap-1">
          <button
            type="button"
            class="icon-button"
            :aria-label="t('common.copy')"
            :data-tooltip="t('common.copy')"
            @click="emit('copy', item)"
          >
            <ClipboardCopy class="size-3.5" />
          </button>

          <button
            type="button"
            class="icon-button"
            :aria-label="t('clip.expand')"
            :data-tooltip="t('clip.expand')"
            @click="emit('expand', item)"
          >
            <Maximize2 class="size-3.5" />
          </button>

          <button
            type="button"
            class="icon-button"
            :aria-label="t('common.paste')"
            :data-tooltip="t('common.paste')"
            @click="emit('apply', item)"
          >
            <CornerDownLeft class="size-3.5 text-[var(--accent)]" />
          </button>
        </div>
      </div>

      <!-- Inspector Body -->
      <div class="clip-inspector-body">
        <!-- Image Preview Box -->
        <div
          v-if="isImage"
          class="clip-inspector-image-box group relative"
        >
          <img
            :src="imageSrc"
            :alt="t('common.imagePreviewAlt')"
          >
          <div class="absolute bottom-2.5 right-2.5 flex items-center gap-1.5">
            <button
              type="button"
              class="inline-flex items-center gap-1 whitespace-nowrap rounded-md bg-black/75 px-2.5 py-1.5 text-xs font-medium text-white shadow-lg backdrop-blur hover:bg-black/90 transition-transform active:scale-95"
              :aria-label="t('clip.recognizeText')"
              :data-tooltip="t('clip.recognizeText')"
              @click="emit('ocr', item)"
            >
              <ScanText class="size-3.5" />
              <span>{{ t("clip.recognizeText") }}</span>
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-1 whitespace-nowrap rounded-md bg-black/75 px-2.5 py-1.5 text-xs font-medium text-white shadow-lg backdrop-blur hover:bg-black/90 transition-transform active:scale-95"
              @click="emit('expand', item)"
            >
              <Eye class="size-3.5" />
              <span>{{ t("clip.expand") }}</span>
            </button>
          </div>
        </div>

        <!-- Color Preview Box with Format Converter Matrix -->
        <div
          v-else-if="isColor"
          class="clip-inspector-color-card"
        >
          <div class="clip-inspector-color-header">
            <div
              class="clip-inspector-color-preview"
              :style="{ backgroundColor: colorValue }"
            />
            <div class="flex flex-col gap-1 min-w-0 flex-1">
              <span class="font-mono text-sm font-semibold text-[var(--text-1)]">{{ colorValue }}</span>
              <span class="text-xs text-[var(--text-3)]">{{ typeLabel("color") }}</span>
            </div>
          </div>

          <div class="color-matrix-grid">
            <button
              v-for="fmt in colorFormats"
              :key="fmt.label"
              type="button"
              class="color-matrix-item"
              :title="`Click to copy ${fmt.label}`"
              @click="copyFormatValue(fmt.value, fmt.label)"
            >
              <span class="color-matrix-item-label">{{ fmt.label }}</span>
              <span
                v-if="copiedFormat === fmt.label"
                class="inline-flex items-center gap-1 text-[var(--success)] font-semibold"
              >
                <Check class="size-3" />
                <span>{{ t("common.copy") }}</span>
              </span>
              <span
                v-else
                class="color-matrix-item-val truncate max-w-[120px]"
              >{{ fmt.value }}</span>
            </button>
          </div>
        </div>

        <!-- Link Preview Box -->
        <div
          v-else-if="isLink"
          class="flex flex-col gap-2 rounded-md border border-[var(--border-hairline)] bg-[var(--surface-code)] p-3.5"
        >
          <div class="flex items-center gap-2 text-xs font-semibold text-[var(--text-1)]">
            <Globe class="size-4 text-[var(--accent)] shrink-0" />
            <span class="truncate">{{ linkHostname }}</span>
          </div>
          <div class="font-mono text-xs text-[var(--text-2)] break-all select-text leading-relaxed">
            {{ item.text }}
          </div>
          <a
            :href="item.text"
            target="_blank"
            rel="noopener noreferrer"
            class="btn-ghost self-start text-xs inline-flex items-center gap-1.5 mt-1 border border-[var(--border)]"
          >
            <ExternalLink class="size-3.5" />
            <span>{{ typeLabel("link") }}</span>
          </a>
        </div>

        <!-- Text / Code Preview Box with Gutter Header -->
        <div
          v-else
          class="clip-inspector-content-box"
        >
          <div class="clip-inspector-code-header tabular-nums">
            <span>{{ isCode ? t("detail.codeSnippet") : t("detail.plainText") }}</span>
            <span>{{ item ? textStats(item.text) : "" }} · {{ lineCountText(lines) }}</span>
          </div>
          <pre class="clip-inspector-code-content">{{ item.text }}</pre>
        </div>

        <!-- Meta Info Card Footer with Chips -->
        <div class="clip-inspector-meta-card tabular-nums">
          <div class="clip-inspector-meta-item">
            <span class="clip-inspector-meta-label">{{ t("common.size") }}</span>
            <span class="clip-inspector-meta-val">{{ clipMetricText(item.clipType, item.text, item.previewText) }}</span>
          </div>

          <div
            v-if="!isImage"
            class="clip-inspector-meta-item"
          >
            <span class="clip-inspector-meta-label">{{ t("common.lines") }}</span>
            <span class="clip-inspector-meta-val">{{ lineCountText(lines) }}</span>
          </div>

          <div class="clip-inspector-meta-item col-span-2">
            <span class="clip-inspector-meta-label">{{ t("detail.copiedAt") }}</span>
            <span class="clip-inspector-meta-val">{{ formatTime(displayTime) }}</span>
          </div>
        </div>
      </div>
    </template>

    <!-- Empty Inspector Placeholder -->
    <div
      v-else
      class="flex h-full flex-col items-center justify-center gap-2 px-6 text-center text-[var(--text-3)]"
    >
      <Clipboard class="size-8 stroke-[1.5] opacity-50" />
      <p class="text-xs">
        {{ t("detail.noSelection") }}
      </p>
    </div>
  </aside>
</template>
