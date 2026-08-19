import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,
  // Override default ignores of eslint-config-next.
  globalIgnores([
    // Default ignores of eslint-config-next:
    ".next/**",
    "out/**",
    "build/**",
    "next-env.d.ts",
    // assistant-ui registry 底稿（第三方源码，仅作 fork 参考，不适用本项目规则）
    "components/assistant-ui/**",
  ]),
  {
    // 硬性约定：整页/整区加载一律 LoadingState 转圈，业务组件禁用骨架屏
    //（见 AGENTS.md「硬性约定」第 1 条）
    files: ["app/**/*.tsx", "components/stratum/**/*.tsx", "components/chrome/**/*.tsx"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          paths: [
            {
              name: "@/components/ui/skeleton",
              message:
                "整页/整区加载一律使用 LoadingState 转圈（components/stratum/studio/primitives.tsx）；骨架屏违反项目硬性约定。",
            },
          ],
        },
      ],
    },
  },
]);

export default eslintConfig;
