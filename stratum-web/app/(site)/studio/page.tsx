import { Suspense } from "react"

import { StudioDashboard } from "@/components/stratum/studio/studio-dashboard"

export default function StudioPage() {
  return (
    <Suspense>
      <StudioDashboard />
    </Suspense>
  )
}
