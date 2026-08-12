"use client"

import { memo, useCallback, useRef, useState } from "react"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"
import {
  CloudOff,
  CirclePause,
  Play,
  RotateCcw,
  TriangleAlert,
} from "lucide-react"

import { Notice } from "@/components/stratum/conversation/notice"
import { Button } from "@/components/ui/button"
import { presentConversationError } from "@/features/agent-conversation/error-notice"
import type { ConversationState } from "@/features/agent-conversation/types"
import type { ApiError } from "@/lib/stratum/api"
import { MOTION_DURATION, MOTION_EASE, motionDuration } from "@/lib/motion"

gsap.registerPlugin(useGSAP)

/**
 * 会话状态提示面（composer 上方、文档流内的小横幅），统一走 Notice
 * （左对齐 tinted 横幅；warning 黄 = 需要知晓的进行中/降级状态）：
 * - ResumeNotice：`resume_required` advisory —— Turn 仍为 running 但当前进程
 *   未托管；显式「恢复执行」按钮，绝不自动 resume。
 * - RealtimeDegradedNotice：NATS 不可用导致 realtime 降级；核心命令与 PG
 *   reconcile 继续工作，不做成错误横幅。
 * - AnimatedConversationErrorNotice：命令/连接错误不进入正文；恢复后先播放
 *   退场再卸载，避免状态提示闪断。
 */

type RenderedError = {
  key: string
  title: string
  description: string
}

export function AnimatedConversationErrorNotice({
  phase,
  error,
  onReconnect,
}: {
  phase: ConversationState["phase"]
  error: ApiError | null
  onReconnect: () => void
}) {
  const presentation = presentConversationError(phase, error)
  const next =
    presentation === null || error === null
      ? null
      : {
          key: `${phase}:${error.code}:${error.status}:${presentation.title}`,
          ...presentation,
        }
  const nextKey = next?.key ?? null
  const [presence, setPresence] = useState<{
    observedKey: string | null
    rendered: RenderedError | null
  }>({ observedKey: nextKey, rendered: next })

  // derive-state-during-render：错误消失时保留最后一份文案直到 GSAP 退场
  // 完成；新错误到达则立即替换，并打断正在进行的退场。
  if (presence.observedKey !== nextKey) {
    setPresence({
      observedKey: nextKey,
      rendered: next ?? presence.rendered,
    })
  }

  const rootRef = useRef<HTMLDivElement>(null)
  const mountedKeyRef = useRef<string | null>(null)
  const leaving = next === null && presence.rendered !== null
  const handleExitDone = useCallback(() => {
    mountedKeyRef.current = null
    setPresence((current) =>
      current.observedKey === null ? { ...current, rendered: null } : current
    )
  }, [])

  useGSAP(
    () => {
      const element = rootRef.current
      const rendered = presence.rendered
      if (!element || !rendered) return

      if (leaving) {
        gsap.to(element, {
          y: -8,
          height: 0,
          marginBottom: 0,
          autoAlpha: 0,
          duration: motionDuration(MOTION_DURATION.fast),
          ease: MOTION_EASE.exit,
          overwrite: "auto",
          onComplete: handleExitDone,
        })
        return
      }

      if (mountedKeyRef.current === null) {
        gsap.set(element, {
          y: 8,
          height: 0,
          marginBottom: 0,
          autoAlpha: 0,
        })
      }
      gsap.to(element, {
        y: 0,
        height: "auto",
        marginBottom: "0.5rem",
        autoAlpha: 1,
        duration: motionDuration(MOTION_DURATION.base),
        ease: MOTION_EASE.enter,
        overwrite: "auto",
        onComplete: () =>
          gsap.set(element, {
            clearProps: "height,marginBottom,opacity,transform,visibility",
          }),
      })
      mountedKeyRef.current = rendered.key
    },
    {
      scope: rootRef,
      dependencies: [leaving, presence.rendered?.key, handleExitDone],
      revertOnUpdate: true,
    }
  )

  if (presence.rendered === null) return null

  return (
    <div ref={rootRef} className="mb-2 overflow-hidden">
      <Notice
        tone="error"
        icon={TriangleAlert}
        action={
          phase === "connection_error" && !leaving ? (
            <Button
              variant="outline"
              size="xs"
              className="shrink-0 rounded-full border-destructive/40 text-destructive hover:bg-destructive/10 hover:text-destructive"
              onClick={onReconnect}
            >
              <RotateCcw aria-hidden />
              重新连接
            </Button>
          ) : undefined
        }
      >
        <p className="font-medium text-foreground">{presence.rendered.title}</p>
        <p className="mt-0.5 text-xs leading-relaxed">
          {presence.rendered.description}
        </p>
      </Notice>
    </div>
  )
}

export const ResumeNotice = memo(function ResumeNotice({
  onResume,
  className,
}: {
  onResume: () => void
  className?: string
}) {
  return (
    <Notice
      tone="warning"
      icon={CirclePause}
      className={className}
      action={
        <Button size="xs" className="shrink-0 rounded-full" onClick={onResume}>
          <Play aria-hidden />
          恢复执行
        </Button>
      }
    >
      执行在当前环境已暂停，恢复后将继续。
    </Notice>
  )
})

export const RealtimeDegradedNotice = memo(function RealtimeDegradedNotice({
  className,
}: {
  className?: string
}) {
  return (
    <Notice tone="warning" icon={CloudOff} className={className}>
      实时连接降级，将定期同步最新进展。
    </Notice>
  )
})
