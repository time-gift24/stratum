"use client"

import type { OntologyResource } from "@/lib/stratum/api"

// A successful create already returns the canonical document and its strong ETag.
// Keep that response for the immediately-following client navigation so the editor
// does not repeat the read. A full reload naturally falls back to GET.
let pendingCreatedOntology: OntologyResource | null = null

export function stageCreatedOntology(resource: OntologyResource): void {
  pendingCreatedOntology = resource
}

export function readCreatedOntologyHandoff(
  ontologyId: string
): OntologyResource | null {
  const resource = pendingCreatedOntology
  if (resource?.document.id === ontologyId) return resource
  pendingCreatedOntology = null
  return null
}

export function finishCreatedOntologyHandoff(ontologyId: string): void {
  if (pendingCreatedOntology?.document.id === ontologyId)
    pendingCreatedOntology = null
}
