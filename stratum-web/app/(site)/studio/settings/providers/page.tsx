import { Suspense } from "react"

import { SettingsList } from "@/components/stratum/studio/settings-list"

export default function ProvidersPage() {
  return (
    <Suspense>
      <SettingsList kind="providers" />
    </Suspense>
  )
}
