import { Suspense } from "react"
import { notFound } from "next/navigation"

import { ModelEditor } from "@/components/stratum/studio/model-editor"
import type { ProviderKind } from "@/lib/stratum/api"

export default async function NewModelPage({
  params,
}: {
  params: Promise<{ provider: string }>
}) {
  const { provider } = await params
  if (provider !== "openai" && provider !== "deepseek") notFound()
  return (
    <Suspense>
      <ModelEditor provider={provider as ProviderKind} />
    </Suspense>
  )
}
