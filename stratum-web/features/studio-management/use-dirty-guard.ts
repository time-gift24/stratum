"use client"

import { useCallback, useEffect } from "react"

const MESSAGE = "有未保存的更改，确定离开吗？"

export function useDirtyGuard(dirty: boolean): () => boolean {
  const confirmNavigation = useCallback(
    () => !dirty || window.confirm(MESSAGE),
    [dirty]
  )

  useEffect(() => {
    if (!dirty) return

    const guardedUrl = window.location.href

    const beforeUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault()
    }
    const click = (event: MouseEvent) => {
      const target = event.target
      if (!(target instanceof Element)) return
      const anchor = target.closest("a[href]")
      if (!(anchor instanceof HTMLAnchorElement)) return
      if (anchor.target === "_blank" || anchor.href === window.location.href)
        return
      if (!confirmNavigation()) {
        event.preventDefault()
        event.stopPropagation()
      }
    }
    const popState = () => {
      if (confirmNavigation()) return
      window.history.pushState(window.history.state, "", guardedUrl)
    }

    window.addEventListener("beforeunload", beforeUnload)
    window.addEventListener("popstate", popState)
    document.addEventListener("click", click, true)
    return () => {
      window.removeEventListener("beforeunload", beforeUnload)
      window.removeEventListener("popstate", popState)
      document.removeEventListener("click", click, true)
    }
  }, [confirmNavigation, dirty])

  return confirmNavigation
}
