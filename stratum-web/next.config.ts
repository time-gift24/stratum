import { fileURLToPath } from "node:url"

import type { NextConfig } from "next"

const nextConfig: NextConfig = {
  // 开发环境允许通过 127.0.0.1 访问 dev 资源（HMR websocket 等）
  allowedDevOrigins: ["127.0.0.1", "localhost"],
  turbopack: {
    root: fileURLToPath(new URL(".", import.meta.url)),
  },
  experimental: {
    optimizePackageImports: ["@xyflow/react"],
    // 动态页客户端 router 缓存 30s：默认 0 导致每次导航后视口内全部 Link
    // （顶导 + 设置页签）重新预取，RSC 响应风暴挤占主线程、切换掉帧
    staleTimes: { dynamic: 30 },
  },
}

export default nextConfig
