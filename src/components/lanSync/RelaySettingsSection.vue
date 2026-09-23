<script setup lang="ts">
import { ref } from "vue";
import { ChevronDown, ChevronRight } from "lucide-vue-next";
import { t } from "../../i18n";

defineProps<{
  relayInput: string;
  relaySaved: boolean;
  relayError: string | null;
  savingRelay: boolean;
  autoPushMaster: boolean;
  autoPushNotify: boolean;
  autoPushError: string | null;
}>();

const emit = defineEmits<{
  "update:relayInput": [value: string];
  "update:autoPushMaster": [value: boolean];
  "update:autoPushNotify": [value: boolean];
  saveRelay: [];
  saveAutoPush: [];
}>();

// 折叠态仅影响本 section 展示，留在组件内。
const relayOpen = ref(false);

// 先上抛开关值再触发保存（对齐原 v-model + @change 的执行顺序），
// 面板保存时读到的已是新值。
function onMasterChange(event: Event) {
  emit("update:autoPushMaster", (event.target as HTMLInputElement).checked);
  emit("saveAutoPush");
}

function onNotifyChange(event: Event) {
  emit("update:autoPushNotify", (event.target as HTMLInputElement).checked);
  emit("saveAutoPush");
}
</script>

<template>
  <!-- 传输设置（折叠区） -->
  <section class="lan-section lan-section-footer">
    <button
      type="button"
      class="lan-collapsible-header"
      :aria-expanded="relayOpen"
      @click="relayOpen = !relayOpen"
    >
      <ChevronDown
        v-if="relayOpen"
        :size="14"
      />
      <ChevronRight
        v-else
        :size="14"
      />
      {{ t("deviceSync.relay.title") }}
    </button>
    <div
      v-if="relayOpen"
      class="lan-relay-body"
    >
      <label
        class="lan-label"
        for="lan-relay-input"
      >{{ t("deviceSync.relay.label") }}</label>
      <div class="lan-ticket-row">
        <input
          id="lan-relay-input"
          :value="relayInput"
          class="lan-input"
          type="text"
          :placeholder="t('deviceSync.relay.placeholder')"
          @input="emit('update:relayInput', ($event.target as HTMLInputElement).value)"
        >
        <button
          type="button"
          class="lan-button"
          :disabled="savingRelay"
          @click="emit('saveRelay')"
        >
          {{ t("common.save") }}
        </button>
      </div>
      <p
        v-if="relaySaved"
        class="lan-hint"
      >
        {{ t("deviceSync.relay.restartHint") }}
      </p>
      <p
        v-if="relayError"
        class="lan-error"
      >
        {{ relayError }}
      </p>
      <label class="lan-setting-row">
        <input
          type="checkbox"
          :checked="autoPushMaster"
          @change="onMasterChange"
        >
        <span>{{ t("deviceSync.autoPush.master") }}</span>
      </label>
      <label class="lan-setting-row">
        <input
          type="checkbox"
          :checked="autoPushNotify"
          @change="onNotifyChange"
        >
        <span>{{ t("deviceSync.autoPush.notify") }}</span>
      </label>
      <p
        v-if="autoPushError"
        class="lan-error"
      >
        {{ autoPushError }}
      </p>
    </div>
  </section>
</template>
