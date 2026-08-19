import { Suspense } from "react"

import { ModelEditor } from "@/components/stratum/studio/model-editor"

export default async function ModelPage({
  params,
}: {
  params: Promise<{ model_id: string }>
}) {
  const { model_id: modelId } = await params
  return (
    <Suspense>
      <ModelEditor key={modelId} modelId={modelId} />
    </Suspense>
  )
}
