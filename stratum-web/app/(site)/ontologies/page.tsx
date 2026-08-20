"use client"

import { useState } from "react"

import { OntologyCreateDialog } from "@/components/stratum/ontology/ontology-create-dialog"
import { OntologyDeleteDialog } from "@/components/stratum/ontology/ontology-delete-dialog"
import { OntologyList } from "@/components/stratum/ontology/ontology-list"
import { PageShell } from "@/components/stratum/studio/primitives"
import type { OntologySummary } from "@/features/ontology-editor/types"
import { useOntologyList } from "@/hooks/use-ontology-list"

/**
 * Ontology 列表页：薄页面，数据经 useOntologyList 获取后以 props 下发。
 * 顶部避让由 PageShell 统一提供（pt-24 sm:pt-28，比对话页更松）。
 */
export default function OntologiesPage() {
  const { state, api, search, loadPage, setSearch, reload } = useOntologyList()
  const [createOpen, setCreateOpen] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<OntologySummary | null>(null)

  // 删除等写操作后的列表刷新：若删的是当前页最后一项且不在第一页，回退一页
  const handleListChanged = () => {
    if (
      state.phase === "ready" &&
      state.page > 1 &&
      state.result.data.length <= 1
    ) {
      loadPage(state.page - 1)
      return
    }
    reload()
  }

  return (
    <PageShell>
      <OntologyList
        state={state}
        query={search}
        onPageChange={loadPage}
        onRetry={reload}
        onSearch={setSearch}
        onRequestCreate={() => setCreateOpen(true)}
        onRequestDelete={setDeleteTarget}
      />
      <OntologyCreateDialog
        api={api}
        open={createOpen}
        onOpenChange={setCreateOpen}
      />
      <OntologyDeleteDialog
        api={api}
        ontology={deleteTarget}
        onClose={() => setDeleteTarget(null)}
        onListChanged={handleListChanged}
      />
    </PageShell>
  )
}
