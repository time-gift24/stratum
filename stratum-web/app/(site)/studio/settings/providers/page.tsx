import { Suspense } from "react"

import { ProviderList } from "@/components/stratum/studio/provider-list"

export default function ProvidersPage() {
  return (
    <Suspense>
      <ProviderList />
    </Suspense>
  )
}
