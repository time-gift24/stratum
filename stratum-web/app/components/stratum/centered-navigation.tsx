"use client"

import { useEffect, useId, useState, type ReactNode } from "react"
import { AnimatePresence, motion, useReducedMotion } from "motion/react"
import { ChevronDown, Menu, X, type LucideIcon } from "lucide-react"

import { glassSurface } from "~/components/stratum/glass-surface"
import { StratumMark } from "~/components/stratum/stratum-mark"
import { cn } from "~/lib/utils"

export type CenteredNavigationTone =
  | "neutral"
  | "blue"
  | "yellow"
  | "magenta"
  | "green"

export interface CenteredNavigationItem {
  title: string
  description: string
  href: string
  icon: LucideIcon
  tone?: CenteredNavigationTone
}

export interface CenteredNavigationGroup {
  id: string
  label: string
  items: readonly CenteredNavigationItem[]
}

export interface CenteredNavigationLink {
  label: string
  href: string
}

type CenteredNavigationProps = {
  ariaLabel: string
  brandHref?: string
  brandLabel?: string
  groups: readonly CenteredNavigationGroup[]
  links?: readonly CenteredNavigationLink[]
  actionHref?: string
  actionLabel?: string
  openMenuLabel: string
  closeMenuLabel: string
  utility?: ReactNode
  className?: string
}

const NAV_SURFACE = glassSurface({
  surface: "popover",
  elevation: "navigation",
})

const ICON_TONE_CLASS: Record<CenteredNavigationTone, string> = {
  neutral: "bg-foreground/8 text-foreground",
  blue: "bg-chart-1/14 text-chart-1",
  yellow: "bg-chart-2/14 text-chart-2",
  magenta: "bg-chart-3/14 text-chart-3",
  green: "bg-primary/14 text-primary",
}

export function CenteredNavigation({
  ariaLabel,
  brandHref = "/chat",
  brandLabel = "Stratum",
  groups,
  links = [],
  actionHref,
  actionLabel,
  openMenuLabel,
  closeMenuLabel,
  utility,
  className,
}: CenteredNavigationProps) {
  const [activeGroupId, setActiveGroupId] = useState<string | null>(null)
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)
  const navId = useId()
  const reduceMotion = useReducedMotion()
  const activeGroup = groups.find((group) => group.id === activeGroupId)

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return
      setActiveGroupId(null)
      setMobileMenuOpen(false)
    }
    window.addEventListener("keydown", closeOnEscape)
    return () => window.removeEventListener("keydown", closeOnEscape)
  }, [])

  const transition = reduceMotion
    ? { duration: 0.01 }
    : { duration: 0.24, ease: [0.22, 1, 0.36, 1] as const }

  return (
    <nav
      className={cn(
        "fixed top-4 left-1/2 z-(--z-navigation) w-[calc(100%-2rem)] max-w-(--global-nav-width) -translate-x-1/2 before:pointer-events-none before:absolute before:-inset-x-32 before:-top-16 before:-z-10 before:h-48 before:bg-[radial-gradient(ellipse_at_top,color-mix(in_srgb,var(--chart-5)_16%,transparent),transparent_72%)]",
        className
      )}
      aria-label={ariaLabel}
    >
      <DesktopNavigation
        variant="precision"
        activeGroup={activeGroup}
        activeGroupId={activeGroupId}
        actionHref={actionHref}
        actionLabel={actionLabel}
        brandHref={brandHref}
        brandLabel={brandLabel}
        groups={groups}
        links={links}
        navId={`${navId}-precision`}
        reduceMotion={Boolean(reduceMotion)}
        setActiveGroupId={setActiveGroupId}
        transition={transition}
        utility={utility}
      />

      <motion.div
        className="mx-auto max-w-md lg:hidden"
        initial={reduceMotion ? false : { opacity: 0, y: -10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={transition}
      >
        <div
          className={cn(
            NAV_SURFACE,
            "flex h-15 items-center justify-between rounded-xl px-2.5"
          )}
        >
          <BrandLink compact href={brandHref} label={brandLabel} />
          <div className="flex items-center gap-1.5">
            {utility}
            <button
              type="button"
              className="grid size-11 place-items-center rounded-lg text-foreground/80 transition-colors hover:bg-foreground/7 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none [&>svg]:size-5"
              aria-expanded={mobileMenuOpen}
              aria-controls={`${navId}-mobile-panel`}
              aria-label={mobileMenuOpen ? closeMenuLabel : openMenuLabel}
              onClick={() => setMobileMenuOpen((open) => !open)}
            >
              {mobileMenuOpen ? (
                <X aria-hidden="true" />
              ) : (
                <Menu aria-hidden="true" />
              )}
            </button>
          </div>
        </div>
        <AnimatePresence initial={false}>
          {mobileMenuOpen ? (
            <motion.div
              id={`${navId}-mobile-panel`}
              className={cn(NAV_SURFACE, "mt-2 overflow-hidden rounded-xl")}
              initial={reduceMotion ? false : { height: 0, opacity: 0 }}
              animate={{ height: "auto", opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              transition={transition}
            >
              <div className="flex flex-col gap-3 p-3">
                {groups.map((group) => (
                  <section key={group.id} className="space-y-2">
                    <h2 className="px-2 text-xs font-semibold tracking-wide text-muted-foreground uppercase">
                      {group.label}
                    </h2>
                    <div className="grid gap-2">
                      {group.items.map((item) => (
                        <NavigationCard
                          key={item.href}
                          item={item}
                          reduceMotion
                        />
                      ))}
                    </div>
                  </section>
                ))}
                {links.map((link) => (
                  <a
                    key={link.href}
                    className="flex min-h-11 items-center rounded-lg px-3 text-sm font-semibold text-foreground transition-colors hover:bg-foreground/7"
                    href={link.href}
                  >
                    {link.label}
                  </a>
                ))}
                {actionHref && actionLabel ? (
                  <a
                    className="flex min-h-11 items-center justify-center rounded-lg bg-primary px-4 text-sm font-semibold text-primary-foreground"
                    href={actionHref}
                  >
                    {actionLabel}
                  </a>
                ) : null}
              </div>
            </motion.div>
          ) : null}
        </AnimatePresence>
      </motion.div>
    </nav>
  )
}

type DesktopVariant = "original" | "continuous" | "warm" | "precision"

function DesktopNavigation({
  variant,
  activeGroup,
  activeGroupId,
  actionHref,
  actionLabel,
  brandHref,
  brandLabel,
  groups,
  links,
  navId,
  reduceMotion,
  setActiveGroupId,
  transition,
  utility,
}: {
  variant: DesktopVariant
  activeGroup: CenteredNavigationGroup | undefined
  activeGroupId: string | null
  actionHref?: string
  actionLabel?: string
  brandHref: string
  brandLabel: string
  groups: readonly CenteredNavigationGroup[]
  links: readonly CenteredNavigationLink[]
  navId: string
  reduceMotion: boolean
  setActiveGroupId: (groupId: string | null) => void
  transition: { duration: number; ease?: readonly [number, number, number, number] }
  utility?: ReactNode
}) {
  const continuous = variant !== "original"
  return (
    <motion.div
      className={cn(
        "mx-auto hidden w-full lg:block",
        continuous && NAV_SURFACE,
        variant === "continuous" && "overflow-hidden rounded-xl",
        variant === "warm" &&
          "overflow-hidden rounded-2xl bg-popover/72 shadow-[0_28px_70px_color-mix(in_srgb,var(--background)_62%,transparent),inset_0_1px_0_color-mix(in_srgb,var(--foreground)_13%,transparent)]",
        variant === "precision" &&
          "max-w-[31rem] overflow-hidden rounded-xl bg-popover/54 shadow-[0_28px_72px_-28px_color-mix(in_srgb,var(--background)_78%,transparent)]"
      )}
      initial={reduceMotion ? false : { opacity: 0, y: -12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={transition}
      onMouseLeave={() => setActiveGroupId(null)}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) {
          setActiveGroupId(null)
        }
      }}
    >
      <div
        className={cn(
          "flex items-center px-3",
          variant === "original" && NAV_SURFACE,
          variant === "original" && "h-16 rounded-xl",
          variant === "continuous" && "h-16",
          variant === "warm" && "h-[4.25rem] px-3.5",
          variant === "precision" && "h-15 px-2.5"
        )}
      >
        <BrandLink href={brandHref} label={brandLabel} />
        <div className="flex min-w-0 flex-1 items-center justify-center gap-1 px-3">
          {groups.map((group) => {
            const expanded = activeGroupId === group.id
            const panelId = `${navId}-${group.id}`
            return (
              <button
                key={group.id}
                type="button"
                className={cn(
                  "inline-flex min-h-11 min-w-28 items-center justify-center gap-1.5 rounded-lg px-4 text-[0.9375rem] font-semibold text-foreground/78 transition-[background-color,box-shadow,color] duration-200 hover:bg-foreground/6 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
                  expanded &&
                    variant === "continuous" &&
                    "bg-foreground/7 text-foreground shadow-[inset_0_1px_0_color-mix(in_srgb,var(--foreground)_9%,transparent)]",
                  expanded &&
                    variant === "warm" &&
                    "bg-primary/9 text-foreground shadow-[inset_0_1px_0_color-mix(in_srgb,var(--primary)_22%,transparent)]",
                  expanded &&
                    variant === "precision" &&
                    "bg-primary/10 text-primary ring-1 ring-primary/18"
                )}
                aria-expanded={expanded}
                aria-controls={panelId}
                onMouseEnter={() => setActiveGroupId(group.id)}
                onFocus={() => setActiveGroupId(group.id)}
                onClick={() => setActiveGroupId(expanded ? null : group.id)}
              >
                {group.label}
                <ChevronDown
                  className={cn(
                    "size-4 transition-transform duration-200",
                    expanded && "rotate-180"
                  )}
                  aria-hidden="true"
                />
              </button>
            )
          })}
          {links.map((link) => (
            <a
              key={link.href}
              className="inline-flex min-h-11 min-w-24 items-center justify-center rounded-lg px-4 text-[0.9375rem] font-semibold text-foreground/78 transition-colors duration-200 hover:bg-foreground/6 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
              href={link.href}
              onMouseEnter={() => setActiveGroupId(null)}
              onFocus={() => setActiveGroupId(null)}
            >
              {link.label}
            </a>
          ))}
        </div>
        <div className="flex min-w-24 items-center justify-end gap-1.5">
          {utility}
          {actionHref && actionLabel ? (
            <a
              className="inline-flex min-h-11 items-center rounded-lg bg-primary px-4 text-sm font-semibold text-primary-foreground transition-transform duration-200 hover:-translate-y-0.5"
              href={actionHref}
            >
              {actionLabel}
            </a>
          ) : null}
        </div>
      </div>

      <AnimatePresence initial={false}>
        {activeGroup ? (
          <motion.div
            key={activeGroup.id}
            id={`${navId}-${activeGroup.id}`}
            className={cn(
              "overflow-hidden",
              variant === "original" && NAV_SURFACE,
              variant === "original" && "mt-2 rounded-xl",
              variant === "continuous" && "bg-foreground/[0.025]",
              variant === "warm" && "bg-background/18",
              variant === "precision" && "bg-transparent"
            )}
            initial={
              reduceMotion
                ? false
                : { height: 0, opacity: 0, filter: "blur(4px)" }
            }
            animate={{ height: "auto", opacity: 1, filter: "blur(0px)" }}
            exit={{ height: 0, opacity: 0, filter: "blur(4px)" }}
            transition={transition}
          >
            <div
              className={cn(
                "grid grid-cols-2 gap-2 p-3",
                continuous && "pt-1",
                variant === "warm" && "gap-2.5 p-3.5 pt-1.5",
                variant === "precision" && "gap-1.5 p-2.5 pt-2"
              )}
            >
              {activeGroup.items.map((item, index) => (
                <NavigationCard
                  key={item.href}
                  item={item}
                  index={index}
                  reduceMotion={reduceMotion}
                  variant={variant}
                />
              ))}
            </div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </motion.div>
  )
}

function BrandLink({
  compact = false,
  href,
  label,
}: {
  compact?: boolean
  href: string
  label: string
}) {
  return (
    <a
      className="group/brand flex min-h-11 shrink-0 items-center gap-2.5 rounded-lg px-2 text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
      href={href}
      aria-label={label}
    >
      <StratumMark
        variant="compact"
        className="size-8 drop-shadow-[0_3px_7px_color-mix(in_srgb,var(--primary)_28%,transparent)] transition-[filter,transform] duration-200 group-hover/brand:rotate-6 group-hover/brand:brightness-110 group-focus-visible/brand:rotate-6"
      />
      <span
        className={cn(
          "font-heading text-lg font-semibold tracking-tight whitespace-nowrap",
          compact && "max-sm:hidden"
        )}
      >
        {label}
      </span>
    </a>
  )
}

function NavigationCard({
  item,
  index = 0,
  reduceMotion,
  variant = "original",
}: {
  item: CenteredNavigationItem
  index?: number
  reduceMotion: boolean
  variant?: DesktopVariant
}) {
  const Icon = item.icon
  return (
    <motion.a
      className={cn(
        "group/card flex min-h-18 items-center gap-3 rounded-lg bg-foreground/4 p-3 transition-[background-color,box-shadow,transform] duration-200 hover:-translate-y-0.5 hover:bg-foreground/7 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
        variant === "original" && "hover:shadow-xl",
        variant === "continuous" &&
          "shadow-[inset_0_1px_0_color-mix(in_srgb,var(--foreground)_6%,transparent)] hover:shadow-[0_10px_24px_color-mix(in_srgb,var(--background)_34%,transparent),inset_0_1px_0_color-mix(in_srgb,var(--foreground)_10%,transparent)]",
        variant === "warm" &&
          "rounded-xl bg-foreground/5 hover:bg-foreground/8 hover:shadow-[0_12px_28px_color-mix(in_srgb,var(--background)_40%,transparent)]",
        variant === "precision" &&
          "min-h-16 bg-foreground/[0.035] p-2.5 ring-1 ring-foreground/6 hover:translate-y-0 hover:ring-foreground/12"
      )}
      href={item.href}
      initial={
        reduceMotion || variant === "precision" ? false : { opacity: 0, y: 8 }
      }
      animate={{ opacity: 1, y: 0 }}
      transition={{
        duration: reduceMotion || variant === "precision" ? 0.01 : 0.2,
        delay:
          reduceMotion || variant === "precision" ? 0 : index * 0.045,
        ease: [0.22, 1, 0.36, 1],
      }}
    >
      <span
        className={cn(
          "grid size-11 shrink-0 place-items-center rounded-lg [&>svg]:size-5",
          ICON_TONE_CLASS[item.tone ?? "neutral"]
        )}
      >
        <Icon aria-hidden="true" />
      </span>
      <span className="min-w-0">
        <strong className="block text-[0.9375rem] font-semibold text-foreground">
          {item.title}
        </strong>
        <small className="mt-0.5 block text-sm leading-snug text-muted-foreground">
          {item.description}
        </small>
      </span>
    </motion.a>
  )
}
