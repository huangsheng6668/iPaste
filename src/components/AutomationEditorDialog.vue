<script setup lang="ts">
import { reactive, ref, watch } from "vue";
import { t } from "../i18n";
import type { AutomationAction, AutomationInput } from "../types";

const props = defineProps<{ open: boolean; action: AutomationAction | null }>();
const emit = defineEmits<{ (e: "save", input: AutomationInput): void; (e: "cancel"): void }>();

const form = reactive<AutomationInput>({
  name: "",
  command: "",
  cwd: null,
  confirmBeforeRun: false,
  closePanelOnSuccess: false,
});
const error = ref<string | null>(null);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    error.value = null;
    if (props.action) {
      form.name = props.action.name;
      form.command = props.action.command;
      form.cwd = props.action.cwd;
      form.confirmBeforeRun = props.action.confirmBeforeRun;
      form.closePanelOnSuccess = props.action.closePanelOnSuccess;
    } else {
      form.name = "";
      form.command = "";
      form.cwd = null;
      form.confirmBeforeRun = false;
      form.closePanelOnSuccess = false;
    }
  },
);

function submit() {
  const command = form.command.trim();
  if (!form.name.trim()) {
    error.value = t("automation.nameRequired");
    return;
  }
  if (!command) {
    error.value = t("automation.commandRequired");
    return;
  }
  if ([...command].length > 4000) {
    error.value = t("automation.commandTooLong");
    return;
  }
  error.value = null;
  emit("save", { ...form, command });
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/40"
      @click.self="emit('cancel')"
    >
      <div
        class="w-[420px] max-w-[90vw] rounded-xl bg-white p-5 shadow-xl"
        role="dialog"
        @keydown.esc="emit('cancel')"
      >
        <h2 class="text-base font-semibold text-slate-900">
          {{ action ? t("automation.edit") : t("automation.newAction") }}
        </h2>

        <label class="mt-4 block">
          <span class="text-sm font-medium text-slate-700">{{ t("automation.name") }}</span>
          <input
            v-model="form.name"
            class="mt-1 w-full rounded-lg border border-slate-300 px-3 py-1.5 text-sm focus:border-slate-400 focus:outline-none"
          />
        </label>

        <label class="mt-3 block">
          <span class="text-sm font-medium text-slate-700">{{ t("automation.command") }}</span>
          <textarea
            v-model="form.command"
            rows="3"
            spellcheck="false"
            class="mt-1 w-full resize-none rounded-lg border border-slate-300 px-3 py-1.5 font-mono text-xs focus:border-slate-400 focus:outline-none"
          />
        </label>

        <label class="mt-3 block">
          <span class="text-sm font-medium text-slate-700">{{ t("automation.cwd") }}</span>
          <input
            v-model="form.cwd"
            spellcheck="false"
            placeholder="E:\code\idea\ipaste-new"
            class="mt-1 w-full rounded-lg border border-slate-300 px-3 py-1.5 font-mono text-xs focus:border-slate-400 focus:outline-none"
          />
        </label>

        <label class="mt-3 flex items-center gap-2">
          <input type="checkbox" v-model="form.confirmBeforeRun" class="size-4" />
          <span class="text-sm text-slate-700">{{ t("automation.confirmBeforeRun") }}</span>
        </label>
        <label class="mt-2 flex items-center gap-2">
          <input type="checkbox" v-model="form.closePanelOnSuccess" class="size-4" />
          <span class="text-sm text-slate-700">{{ t("automation.closePanelOnSuccess") }}</span>
        </label>

        <p v-if="error" class="mt-2 text-sm text-red-600">{{ error }}</p>

        <div class="mt-4 flex justify-end gap-2">
          <button
            type="button"
            class="rounded-lg px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100"
            @click="emit('cancel')"
          >
            {{ t("common.cancel") }}
          </button>
          <button
            type="button"
            class="rounded-lg bg-slate-900 px-3 py-1.5 text-sm text-white hover:bg-slate-700"
            @click="submit"
          >
            {{ t("common.save") }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
