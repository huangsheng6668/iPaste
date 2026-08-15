import js from "@eslint/js";
import tseslint from "typescript-eslint";
import vue from "eslint-plugin-vue";

export default [
  { ignores: ["dist/**", "node_modules/**", "src-tauri/**", "scripts/**", "*.config.*"] },
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
