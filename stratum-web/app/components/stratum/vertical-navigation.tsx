"use client"

import { useEffect, useRef, useState } from "react"
import {
  motion,
  useMotionValue,
  useReducedMotion,
  useSpring,
  useTransform,
} from "motion/react"
import type { LucideIcon } from "lucide-react"

import { cn } from "~/lib/utils"

export type VerticalNavigationTone =
  | "neutral"
  | "blue"
  | "yellow"
  | "magenta"
  | "green"

export interface VerticalNavigationItem {
  id: string
  icon: LucideIcon
  label: string
  href?: string
  onSelect?: () => void
  tone?: VerticalNavigationTone
}

type VerticalNavigationProps = {
  items: readonly VerticalNavigationItem[]
  ariaLabel: string
  activeId?: string
  className?: string
}

const ACTIVE_TONE_CLASS: Record<VerticalNavigationTone, string> = {
  neutral: "bg-foreground/10 text-foreground shadow-foreground/5",
  blue: "bg-chart-1/14 text-chart-1 shadow-chart-1/5",
  yellow: "bg-chart-2/14 text-chart-2 shadow-chart-2/5",
  magenta: "bg-chart-3/14 text-chart-3 shadow-chart-3/5",
  green: "bg-primary/14 text-primary shadow-primary/5",
}

export function VerticalNavigation({
  items,
  ariaLabel,
  activeId,
  className,
}: VerticalNavigationProps) {
  const [selectedId, setSelectedId] = useState(activeId ?? items[0]?.id)
  const mouseY = useMotionValue(Number.POSITIVE_INFINITY)
  const reduceMotion = useReducedMotion()

  useEffect(() => {
    if (activeId !== undefined) setSelectedId(activeId)
  }, [activeId])

  useEffect(() => {
    const anchorItems = items.filter((item) => item.href?.startsWith("#"))
    if (anchorItems.length === 0) return
    const itemIds = new Set(anchorItems.map((item) => item.id))
    let animationFrame = 0

    const syncFromScroll = () => {
      const readingLine = window.innerHeight * 0.34
      let currentId = anchorItems[0]?.id
      for (const item of anchorItems) {
        const section = document.getElementById(item.id)
        if (section && section.getBoundingClientRect().top <= readingLine) {
          currentId = item.id
        }
      }
      if (currentId) setSelectedId(currentId)
    }

    const scheduleScrollSync = () => {
      window.cancelAnimationFrame(animationFrame)
      animationFrame = window.requestAnimationFrame(syncFromScroll)
    }

    const syncFromHash = () => {
      const hashId = decodeURIComponent(window.location.hash.slice(1))
      if (itemIds.has(hashId)) setSelectedId(hashId)
      scheduleScrollSync()
    }

    syncFromHash()
    window.addEventListener("hashchange", syncFromHash)
    window.addEventListener("scroll", scheduleScrollSync, { passive: true })
    return () => {
      window.cancelAnimationFrame(animationFrame)
      window.removeEventListener("hashchange", syncFromHash)
      window.removeEventListener("scroll", scheduleScrollSync)
    }
  }, [items])

  return (
    <motion.nav
      aria-label={ariaLabel}
      className={cn(
        "fixed top-(--global-nav-offset) bottom-4 left-4 z-(--z-overlay) flex w-19 items-center max-sm:bottom-auto max-sm:left-2 max-sm:w-15",
        className
      )}
      initial={reduceMotion ? false : { opacity: 0, x: -18 }}
      animate={{ opacity: 1, x: 0 }}
      transition={{ duration: 0.5, ease: [0.22, 1, 0.36, 1] }}
    >
      <div
        className="flex max-h-full w-full flex-col items-center justify-center gap-2"
        onMouseMove={(event) => mouseY.set(event.clientY)}
        onMouseLeave={() => mouseY.set(Number.POSITIVE_INFINITY)}
      >
        {items.map((item, index) => (
          <NavigationItem
            key={item.id}
            item={item}
            index={index}
            mouseY={mouseY}
            reduceMotion={Boolean(reduceMotion)}
            selected={selectedId === item.id}
            onSelect={setSelectedId}
          />
        ))}
      </div>
    </motion.nav>
  )
}

type NavigationItemProps = {
  item: VerticalNavigationItem
  index: number
  mouseY: ReturnType<typeof useMotionValue<number>>
  reduceMotion: boolean
  selected: boolean
  onSelect(id: string): void
}

function NavigationItem({
  item,
  index,
  mouseY,
  reduceMotion,
  selected,
  onSelect,
}: NavigationItemProps) {
  const itemRef = useRef<HTMLDivElement>(null)
  const Icon = item.icon
  const tone = item.tone ?? "neutral"
  const itemClassName = cn(
    "grid size-full place-items-center rounded-lg bg-sidebar-accent/58 text-sidebar-foreground/65 shadow-lg shadow-background/15 outline-hidden transition-[background-color,color,box-shadow,transform] duration-200 hover:-translate-y-0.5 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:ring-2 focus-visible:ring-sidebar-ring [&>svg]:size-5",
    selected && ACTIVE_TONE_CLASS[tone]
  )
  const distance = useTransform(mouseY, (pointerY) => {
    const bounds = itemRef.current?.getBoundingClientRect()
    if (!bounds) return Number.POSITIVE_INFINITY
    return pointerY - bounds.top - bounds.height / 2
  })
  const targetSize = useTransform(
    distance,
    [-144, -72, 0, 72, 144],
    [48, 52, 60, 52, 48]
  )
  const size = useSpring(targetSize, {
    mass: 0.12,
    stiffness: 180,
    damping: 16,
  })

  const content = (
    <>
      <Icon aria-hidden="true" />
      <span className="sr-only">{item.label}</span>
    </>
  )

  return (
    <motion.div
      ref={itemRef}
      className="group relative grid shrink-0 place-items-center"
      style={{
        width: reduceMotion ? 48 : size,
        height: reduceMotion ? 48 : size,
      }}
      initial={reduceMotion ? false : { opacity: 0, y: 14 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{
        duration: 0.32,
        delay: reduceMotion ? 0 : 0.16 + index * 0.06,
        ease: [0.22, 1, 0.36, 1],
      }}
    >
      {item.href ? (
        <a
          className={itemClassName}
          href={item.href}
          aria-label={item.label}
          aria-current={selected ? "location" : undefined}
          onClick={() => {
            onSelect(item.id)
            item.onSelect?.()
          }}
        >
          {content}
        </a>
      ) : (
        <button
          type="button"
          className={itemClassName}
          aria-label={item.label}
          onClick={item.onSelect}
        >
          {content}
        </button>
      )}
      <span
        aria-hidden="true"
        className="pointer-events-none absolute top-1/2 left-full ml-3 -translate-x-1 -translate-y-1/2 rounded-md bg-popover/92 px-2.5 py-1.5 text-xs font-medium whitespace-nowrap text-popover-foreground opacity-0 shadow-lg backdrop-blur-xl transition-[opacity,transform] duration-150 group-focus-within:translate-x-0 group-focus-within:opacity-100 group-hover:translate-x-0 group-hover:opacity-100"
      >
        {item.label}
      </span>
    </motion.div>
  )
}
