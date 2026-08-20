"use client"

import { useCallback, useEffect, useRef } from "react"

const MESSAGE = "有未保存的更改，确定离开吗？"
const HISTORY_GUARD_KEY = "__stratumDirtyGuard"

type DirtyGuard = {
  confirmNavigation(): boolean
  leave(action: () => void, confirm?: boolean): void
}

export function useDirtyGuard(dirty: boolean): DirtyGuard {
  const guardIdRef = useRef<string | null>(null)
  const guardActiveRef = useRef(false)
  const permittedLeaveRef = useRef(false)
  const releasingRef = useRef(false)
  const pendingLeaveRef = useRef<(() => void) | null>(null)

  const confirmNavigation = useCallback(
    () => !dirty || window.confirm(MESSAGE),
    [dirty]
  )

  const removeGuardThen = useCallback((action: () => void) => {
    permittedLeaveRef.current = true
    const guardId = guardIdRef.current
    if (
      guardActiveRef.current &&
      guardId !== null &&
      window.history.state?.[HISTORY_GUARD_KEY] === guardId
    ) {
      guardActiveRef.current = false
      releasingRef.current = true
      pendingLeaveRef.current = action
      window.history.back()
      return
    }
    action()
  }, [])

  const leave = useCallback(
    (action: () => void, confirm = true) => {
      if (confirm && !confirmNavigation()) return
      removeGuardThen(action)
    },
    [confirmNavigation, removeGuardThen]
  )

  useEffect(() => {
    if (!dirty) return

    const guardedUrl = window.location.href
    const guardId = `${Date.now()}-${Math.random()}`
    const currentState = window.history.state
    const guardState =
      typeof currentState === "object" && currentState !== null
        ? { ...currentState, [HISTORY_GUARD_KEY]: guardId }
        : { [HISTORY_GUARD_KEY]: guardId }
    guardIdRef.current = guardId
    guardActiveRef.current = true
    permittedLeaveRef.current = false
    window.history.pushState(guardState, "", guardedUrl)
    let restoringGuard = false
    let leavingHistory = false
    let skipRepeatedAnchor = false

    const beforeUnload = (event: BeforeUnloadEvent) => {
      if (permittedLeaveRef.current) return
      event.preventDefault()
    }
    const click = (event: MouseEvent) => {
      if (skipRepeatedAnchor) {
        skipRepeatedAnchor = false
        return
      }
      if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey)
        return
      const target = event.target
      if (!(target instanceof Element)) return
      const anchor = target.closest("a[href]")
      if (!(anchor instanceof HTMLAnchorElement)) return
      if (anchor.target === "_blank" || anchor.href === window.location.href)
        return

      event.preventDefault()
      event.stopPropagation()
      if (!confirmNavigation()) return
      removeGuardThen(() => {
        skipRepeatedAnchor = true
        anchor.click()
      })
    }
    const popState = () => {
      if (leavingHistory) return
      if (releasingRef.current) {
        releasingRef.current = false
        guardIdRef.current = null
        const pendingLeave = pendingLeaveRef.current
        pendingLeaveRef.current = null
        pendingLeave?.()
        return
      }
      if (restoringGuard) {
        restoringGuard = false
        return
      }
      if (confirmNavigation()) {
        guardActiveRef.current = false
        guardIdRef.current = null
        permittedLeaveRef.current = true
        leavingHistory = true
        window.setTimeout(() => window.history.back(), 0)
        return
      }
      restoringGuard = true
      window.history.go(1)
    }

    window.addEventListener("beforeunload", beforeUnload)
    window.addEventListener("popstate", popState)
    document.addEventListener("click", click, true)
    return () => {
      window.removeEventListener("beforeunload", beforeUnload)
      window.removeEventListener("popstate", popState)
      document.removeEventListener("click", click, true)
      if (
        guardActiveRef.current &&
        window.history.state?.[HISTORY_GUARD_KEY] === guardId
      ) {
        guardActiveRef.current = false
        guardIdRef.current = null
        window.history.back()
      }
    }
  }, [confirmNavigation, dirty, removeGuardThen])

  return { confirmNavigation, leave }
}
