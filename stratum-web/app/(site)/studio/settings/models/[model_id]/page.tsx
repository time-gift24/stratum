import { Suspense } from "react"

import { ModelEditor } from "@/components/stratum/studio/model-editor"

export default async function ModelPage({
  params,
}: {
  params: Promise<{ model_id: string }>
}) {
  const { model_id: modelId } = await params
  // 列表侧 href 做过 encodeURIComponent（model_id 含 "provider:name" 的冒号）；
  // params 到此仍是编码形态，先解码再交给编辑器拆分
  return (
    <Suspense>
      <ModelEditor modelId={decodeURIComponent(modelId)} />
    </Suspense>
  )
}
