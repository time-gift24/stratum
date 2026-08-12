import { Suspense } from "react"

import { SettingsList } from "@/components/stratum/studio/settings-list"

export default function ModelsPage() {
  return (
    <Suspense>
      <SettingsList kind="models" />
    </Suspense>
  )
}
