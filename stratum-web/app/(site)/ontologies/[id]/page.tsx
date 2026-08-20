import type { Metadata } from "next"

import { OntologyEditorWorkspace } from "@/components/stratum/ontology/ontology-editor-workspace"

export const metadata: Metadata = {
  title: "Ontology 编辑器 - Stratum",
}

/** Ontology 画布编辑器页：薄页面，仅解析路由参数并挂载编辑器工作区（满铺 h-svh，导航常开悬浮，编辑器 chrome 避让顶栏）。 */
export default async function OntologyEditorPage({
  params,
}: {
  params: Promise<{ id: string }>
}) {
  const { id } = await params
  return (
    <div className="h-svh font-sans">
      <OntologyEditorWorkspace ontologyId={id} />
    </div>
  )
}
