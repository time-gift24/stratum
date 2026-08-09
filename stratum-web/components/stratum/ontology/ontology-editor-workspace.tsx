"use client"

import dynamic from "next/dynamic"

/**
 * OntologyEditorWorkspace —— 编辑器页外壳：动态加载画布编辑器
 * （SSR 关闭，xyflow 库与样式表不进首屏），加载中显示同形状骨架。
 */

function EditorSkeleton() {
  return (
    <div
      aria-hidden
      className="h-full w-full animate-pulse bg-muted/50 motion-reduce:animate-none"
    />
  )
}

const OntologyEditor = dynamic(
  () =>
    import("@/components/stratum/ontology/ontology-editor").then(
      (mod) => mod.OntologyEditor
    ),
  { ssr: false, loading: () => <EditorSkeleton /> }
)

export function OntologyEditorWorkspace({ ontologyId }: { ontologyId: string }) {
  return (
    <div
      data-slot="ontology-editor-workspace"
      className="h-full w-full bg-background"
    >
      <OntologyEditor ontologyId={ontologyId} />
    </div>
  )
}
