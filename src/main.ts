import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { installTooltipOverflowGuard } from "./lib/tooltips";
import { initTheme } from "./lib/theme";
import "./styles/main.css";

installTooltipOverflowGuard();
initTheme();

createApp(App).use(createPinia()).mount("#app");
