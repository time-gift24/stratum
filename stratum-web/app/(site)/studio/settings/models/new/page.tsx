import { Suspense } from "react"

import { ModelEditor } from "@/components/stratum/studio/model-editor"

export default function NewModelPage() {
  return (
    <Suspense>
      <ModelEditor />
    </Suspense>
  )
}
