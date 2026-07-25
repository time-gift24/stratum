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

interface VerticalNavigationProps {
  items: readonly VerticalNavigationItem[]
  ariaLabel: string
  activeId?: string
  className?: string
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
    <>
      <motion.nav
        aria-label={ariaLabel}
        className={cn("vertical-navigation", className)}
        initial={reduceMotion ? false : { opacity: 0, x: -18 }}
        animate={{ opacity: 1, x: 0 }}
        transition={{ duration: 0.5, ease: [0.22, 1, 0.36, 1] }}
      >
        <div
          className="vertical-navigation__dock"
          onMouseMove={(event) => mouseY.set(event.clientY)}
          onMouseLeave={() => mouseY.set(Number.POSITIVE_INFINITY)}
        >
          {items.map((item, index) => (
            <DesktopNavigationItem
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
    </>
  )
}

interface DesktopNavigationItemProps {
  item: VerticalNavigationItem
  index: number
  mouseY: ReturnType<typeof useMotionValue<number>>
  reduceMotion: boolean
  selected: boolean
  onSelect: (id: string) => void
}

function DesktopNavigationItem({
  item,
  index,
  mouseY,
  reduceMotion,
  selected,
  onSelect,
}: DesktopNavigationItemProps) {
  const itemRef = useRef<HTMLDivElement>(null)
  const Icon = item.icon
  const tooltipId = `vertical-navigation-${item.id}-tooltip`

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

  return (
    <motion.div
      ref={itemRef}
      className="vertical-navigation__item-shell"
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
          className="vertical-navigation__item"
          href={item.href}
          aria-label={item.label}
          aria-current={selected ? "location" : undefined}
          data-active={selected || undefined}
          data-tone={item.tone ?? "neutral"}
          onClick={() => {
            onSelect(item.id)
            item.onSelect?.()
          }}
        >
          <Icon aria-hidden="true" />
        </a>
      ) : (
        <button
          type="button"
          className="vertical-navigation__item"
          aria-label={item.label}
          data-tone={item.tone ?? "neutral"}
          onClick={item.onSelect}
        >
          <Icon aria-hidden="true" />
        </button>
      )}

      <span
        id={tooltipId}
        aria-hidden="true"
        className="vertical-navigation__tooltip"
      >
        {item.label}
      </span>
    </motion.div>
  )
}

export default VerticalNavigation
