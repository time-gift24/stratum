import { Suspense } from "react"

import { ProviderEditor } from "@/components/stratum/studio/provider-editor"

export default async function ProviderPage({
  params,
}: {
  params: Promise<{ provider: string }>
}) {
  const { provider } = await params
  return (
    <Suspense>
      <ProviderEditor key={provider} provider={provider} />
    </Suspense>
  )
}
