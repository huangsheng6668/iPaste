/// <reference types="vite/client" />

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  // 用具体类型而非 {}/any：既满足 eslint 严格门禁，又保留 .vue 导入的最小形状。
  const component: DefineComponent<Record<string, unknown>, Record<string, unknown>, unknown>;
  export default component;
}
