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
      class="dialog-backdrop"
      @click.self="emit('cancel')"
    >
      <div
        class="dialog-panel"
        role="dialog"
        @keydown.esc="emit('cancel')"
      >
        <div class="dialog-body">
          <h2 class="dialog-title">
            {{ action ? t("automation.edit") : t("automation.newAction") }}
          </h2>

          <label class="mt-4 block">
            <span class="dialog-label">{{ t("automation.name") }}</span>
            <input
              v-model="form.name"
              class="dialog-input mt-1.5 text-sm"
            >
          </label>

          <label class="mt-3 block">
            <span class="dialog-label">{{ t("automation.command") }}</span>
            <textarea
              v-model="form.command"
              rows="3"
              spellcheck="false"
              class="dialog-input mt-1.5 resize-none font-mono text-xs"
            />
          </label>

          <label class="mt-3 block">
            <span class="dialog-label">{{ t("automation.cwd") }}</span>
            <input
              v-model="form.cwd"
              spellcheck="false"
              placeholder="E:\code\idea\ipaste-new"
              class="dialog-input mt-1.5 font-mono text-xs"
            >
          </label>

          <label class="mt-3 flex items-center gap-2">
            <input
              v-model="form.confirmBeforeRun"
              type="checkbox"
              class="dialog-checkbox"
            >
            <span class="text-sm text-slate-700">{{ t("automation.confirmBeforeRun") }}</span>
          </label>
          <label class="mt-2 flex items-center gap-2">
            <input
              v-model="form.closePanelOnSuccess"
              type="checkbox"
              class="dialog-checkbox"
            >
            <span class="text-sm text-slate-700">{{ t("automation.closePanelOnSuccess") }}</span>
          </label>

          <p
            v-if="error"
            class="dialog-error mt-3"
          >
            {{ error }}
          </p>
        </div>

        <div class="dialog-footer">
          <button
            type="button"
            class="btn-ghost"
            @click="emit('cancel')"
          >
            {{ t("common.cancel") }}
          </button>
          <button
            type="button"
            class="btn-primary"
            @click="submit"
          >
            {{ t("common.save") }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
