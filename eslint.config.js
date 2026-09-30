import js from "@eslint/js";
import tseslint from "typescript-eslint";
import vue from "eslint-plugin-vue";

// ESLint 的 ignores 不会自动继承 .gitignore，必须显式列出。
// 漏掉的下场是本地 lint 直接失败（例如 .superdesign/tmp/*.cjs 会报 no-undef），
// 而 CI 上这些目录不存在、lint 反而通过——本地与 CI 行为不一致最难排查。
// 尤其注意 .worktrees/：里面是整份项目副本，漏了会被重复扫描一遍。
const GITIGNORED_LOCAL_DIRS = [
  "dist/**",
  "dist-ssr/**",
  "node_modules/**",
  "src-tauri/**",
  "scripts/**",
  "*.config.*",
  // 本地 AI 工具/工作流状态（见 .gitignore）
  ".superdesign/**",
  ".superpowers/**",
  "docs/superpowers/**",
  ".reasonix/**",
  ".zcode/**",
  ".codegraph/**",
  ".mimosa/**",
  // 隔离开发用的 worktree 与一次性沙箱
  ".worktrees/**",
  "ocr-spike/**",
  ".wrangler/**",
];

export default [
  { ignores: GITIGNORED_LOCAL_DIRS },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  ...vue.configs["flat/recommended"],
  {
    files: ["**/*.{ts,vue}"],
    languageOptions: {
      parserOptions: { parser: tseslint.parser },
    },
    rules: {
      "no-undef": "off",
      "@typescript-eslint/no-unused-vars": ["warn", { argsIgnorePattern: "^_", varsIgnorePattern: "^_" }],
      "@typescript-eslint/no-empty-object-type": "warn",
      "@typescript-eslint/no-explicit-any": "warn",
      "vue/multi-word-component-names": "off",
      "vue/no-v-html": "off",
    },
  },
  {
    files: ["src/**/*.{ts,vue}"],
    ignores: ["src/types/generated/**"],
    rules: {
      "no-restricted-syntax": [
        "error",
        {
          selector: "Literal[value=/ipaste:\\/\\//]",
          message: "禁止手写 ipaste:// 事件名：从 types/generated/events 导入 IPASTE_EVENTS。",
        },
        {
          selector: "VLiteral[value=/ipaste:\\/\\//]",
          message: "禁止在模板中手写 ipaste:// 事件名：从 types/generated/events 导入 IPASTE_EVENTS。",
        },
      ],
    },
  },
];
