import type { NextConfig } from "next"

const nextConfig: NextConfig = {
  // 开发环境允许通过 127.0.0.1 访问 dev 资源（HMR websocket 等）
  allowedDevOrigins: ["127.0.0.1", "localhost"],
}

export default nextConfig
