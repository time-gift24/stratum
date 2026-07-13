"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"

import { DEMO_ONTOLOGY } from "~/lib/ontology-demo"
import {
  OntologyApiError,
  createOntologyApi,
  type OntologyGraph,
  type SchemaDocument,
  type SchemaSource,
  type SourceOptions,
} from "~/lib/ontology-api"

export const DEFAULT_ONTOLOGY_SOURCE: SchemaSource = {
  kind: "tag",
  name: "online",
}

export type OntologyDemoReason =
  | "configuration_missing"
  | "connection_failed"
  | "default_missing"
  | "invalid_response"

export type OntologyWorkspaceError = {
  code: string
  status: number
  message: string
}

type LoadMode = "fallback" | "strict"

type WorkspaceStateBase = {
  source: SchemaSource
  loadMode: LoadMode
}

export type OntologyWorkspaceState =
  | (WorkspaceStateBase & { phase: "loading" })
  | (WorkspaceStateBase & {
      phase: "ready" | "empty"
      graph: OntologyGraph
      schema: SchemaDocument
    })
  | (WorkspaceStateBase & {
      phase: "demo"
      graph: OntologyGraph
      schema: SchemaDocument
      demoReason: OntologyDemoReason
    })
  | (WorkspaceStateBase & {
      phase: "error"
      error: OntologyWorkspaceError
    })

const emptyOptions: SourceOptions = { drafts: [], revisions: [] }

function safeError(error: unknown): OntologyWorkspaceError {
  if (error instanceof OntologyApiError) {
    return { code: error.code, status: error.status, message: error.message }
  }
  return { code: "connection_failed", status: 0, message: "request failed" }
}

function demoReason(error: unknown): OntologyDemoReason {
  if (!(error instanceof OntologyApiError)) return "connection_failed"
  if (error.status === 404) return "default_missing"
  if (error.code === "invalid_response") return "invalid_response"
  return "connection_failed"
}

export function useOntologyWorkspace() {
  const baseUrl = import.meta.env.VITE_WYSE_API_BASE_URL?.trim()
  const api = useMemo(
    () => (baseUrl ? createOntologyApi({ baseUrl }) : undefined),
    [baseUrl]
  )
  const requestRef = useRef(0)
  const abortRef = useRef<AbortController | null>(null)
  const [options, setOptions] = useState<SourceOptions>(emptyOptions)
  const [state, setState] = useState<OntologyWorkspaceState>({
    phase: "loading",
    source: DEFAULT_ONTOLOGY_SOURCE,
    loadMode: "fallback",
  })

  const load = useCallback(
    async (source: SchemaSource, loadMode: LoadMode) => {
      const request = ++requestRef.current
      abortRef.current?.abort()
      const controller = new AbortController()
      abortRef.current = controller
      setState({ phase: "loading", source, loadMode })

      if (!api) {
        setState(
          loadMode === "fallback"
            ? {
                phase: "demo",
                source,
                loadMode,
                graph: DEMO_ONTOLOGY.graph,
                schema: DEMO_ONTOLOGY.schema,
                demoReason: "configuration_missing",
              }
            : {
                phase: "error",
                source,
                loadMode,
                error: {
                  code: "configuration_missing",
                  status: 0,
                  message: "ontology API is not configured",
                },
              }
        )
        return
      }

      try {
        const result = await api.load(source, controller.signal)
        if (request !== requestRef.current) return
        setState({
          phase: result.graph.nodes.length === 0 ? "empty" : "ready",
          source,
          loadMode,
          graph: result.graph,
          schema: result.schema,
        })
      } catch (error) {
        if (controller.signal.aborted || request !== requestRef.current) return
        if (loadMode === "fallback") {
          setState({
            phase: "demo",
            source,
            loadMode,
            graph: DEMO_ONTOLOGY.graph,
            schema: DEMO_ONTOLOGY.schema,
            demoReason: demoReason(error),
          })
          return
        }
        setState({
          phase: "error",
          source,
          loadMode,
          error: safeError(error),
        })
      }
    },
    [api]
  )

  useEffect(() => {
    void load(DEFAULT_ONTOLOGY_SOURCE, "fallback")
    return () => abortRef.current?.abort()
  }, [load])

  useEffect(() => {
    if (!api) {
      setOptions(emptyOptions)
      return
    }
    const controller = new AbortController()
    void api
      .listSources(controller.signal)
      .then(setOptions)
      .catch(() => {
        if (!controller.signal.aborted) setOptions(emptyOptions)
      })
    return () => controller.abort()
  }, [api])

  const selectSource = useCallback(
    (source: SchemaSource) => void load(source, "strict"),
    [load]
  )

  const retry = useCallback(
    () => void load(state.source, state.loadMode),
    [load, state.loadMode, state.source]
  )

  return { state, options, selectSource, retry }
}
