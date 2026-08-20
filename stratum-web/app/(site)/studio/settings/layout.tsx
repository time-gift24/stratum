import { Suspense } from "react"

import { SettingsChrome } from "@/components/stratum/studio/settings-chrome"

/**
 * 设置区共享布局：左侧导航常驻，/studio/settings/** 区内导航只有右侧内容变化。
 */
export default function StudioSettingsLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return (
    <Suspense>
      <SettingsChrome>{children}</SettingsChrome>
    </Suspense>
  )
}
