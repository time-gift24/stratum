import { fileURLToPath } from "node:url"

import { defineConfig } from "vitest/config"

// 协议层单测（lib/stratum + features/agent-conversation）+ 展示层纯函数
// 单测（components/stratum）：纯 node 环境，无 jsdom、无浏览器 API；
// 全部离线 mock，不触网。
export default defineConfig({
  resolve: {
    alias: { "@": fileURLToPath(new URL(".", import.meta.url)) },
  },
  test: {
    environment: "node",
    include: [
      "lib/stratum/**/*.test.ts",
      "features/agent-conversation/**/*.test.ts",
      "features/ontology-editor/**/*.test.ts",
      "hooks/use-ontology-editor.test.ts",
      "components/stratum/**/*.test.ts",
    ],
    exclude: ["node_modules/**", ".next/**"],
  },
})
