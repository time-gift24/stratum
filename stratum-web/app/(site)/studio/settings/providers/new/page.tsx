import { Suspense } from "react"

import { ProviderEditor } from "@/components/stratum/studio/provider-editor"

export default function NewProviderPage() {
  return (
    <Suspense>
      <ProviderEditor />
    </Suspense>
  )
}
