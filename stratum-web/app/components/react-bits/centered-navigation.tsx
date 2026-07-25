"use client"

import { useEffect, useId, useState, type ReactNode } from "react"
import { AnimatePresence, motion, useReducedMotion } from "motion/react"
import { ChevronDown, Menu, X, type LucideIcon } from "lucide-react"

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

interface CenteredNavigationProps {
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
      className={cn("centered-navigation", className)}
      aria-label={ariaLabel}
    >
      <motion.div
        className="centered-navigation__desktop"
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
        <div className="centered-navigation__bar">
          <BrandLink href={brandHref} label={brandLabel} />

          <div className="centered-navigation__links">
            {groups.map((group) => {
              const expanded = activeGroupId === group.id
              const panelId = `${navId}-${group.id}`

              return (
                <button
                  key={group.id}
                  type="button"
                  className="centered-navigation__trigger"
                  aria-expanded={expanded}
                  aria-controls={panelId}
                  onMouseEnter={() => setActiveGroupId(group.id)}
                  onFocus={() => setActiveGroupId(group.id)}
                  onClick={() => setActiveGroupId(expanded ? null : group.id)}
                >
                  {group.label}
                  <ChevronDown aria-hidden="true" />
                </button>
              )
            })}

            {links.map((link) => (
              <a
                key={link.href}
                className="centered-navigation__link"
                href={link.href}
                onMouseEnter={() => setActiveGroupId(null)}
                onFocus={() => setActiveGroupId(null)}
              >
                {link.label}
              </a>
            ))}
          </div>

          <div className="centered-navigation__actions">
            {utility}
            {actionHref && actionLabel ? (
              <a className="centered-navigation__action" href={actionHref}>
                {actionLabel}
              </a>
            ) : null}
          </div>
        </div>

        <AnimatePresence initial={false}>
          {activeGroup && (
            <motion.div
              key={activeGroup.id}
              id={`${navId}-${activeGroup.id}`}
              className="centered-navigation__panel"
              initial={
                reduceMotion
                  ? false
                  : { height: 0, opacity: 0, filter: "blur(4px)" }
              }
              animate={{ height: "auto", opacity: 1, filter: "blur(0px)" }}
              exit={{ height: 0, opacity: 0, filter: "blur(4px)" }}
              transition={transition}
            >
              <div className="centered-navigation__panel-grid">
                {activeGroup.items.map((item, index) => (
                  <NavigationCard
                    key={item.href}
                    item={item}
                    index={index}
                    reduceMotion={Boolean(reduceMotion)}
                  />
                ))}
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </motion.div>

      <motion.div
        className="centered-navigation__mobile"
        initial={reduceMotion ? false : { opacity: 0, y: -10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={transition}
      >
        <div className="centered-navigation__mobile-bar">
          <BrandLink compact href={brandHref} label={brandLabel} />
          <div className="centered-navigation__mobile-actions">
            {utility}
            <button
              type="button"
              className="centered-navigation__menu-button"
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
          {mobileMenuOpen && (
            <motion.div
              id={`${navId}-mobile-panel`}
              className="centered-navigation__mobile-panel"
              initial={reduceMotion ? false : { height: 0, opacity: 0 }}
              animate={{ height: "auto", opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              transition={transition}
            >
              <div className="centered-navigation__mobile-content">
                {groups.map((group) => (
                  <section
                    key={group.id}
                    className="centered-navigation__mobile-group"
                  >
                    <h2>{group.label}</h2>
                    <div>
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
                    className="centered-navigation__mobile-link"
                    href={link.href}
                  >
                    {link.label}
                  </a>
                ))}

                {actionHref && actionLabel ? (
                  <a
                    className="centered-navigation__mobile-action"
                    href={actionHref}
                  >
                    {actionLabel}
                  </a>
                ) : null}
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </motion.div>
    </nav>
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
    <a className="centered-navigation__brand" href={href} aria-label={label}>
      <StratumMark variant="compact" className="size-8" />
      <span
        className={compact ? "centered-navigation__brand-label" : undefined}
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
}: {
  item: CenteredNavigationItem
  index?: number
  reduceMotion: boolean
}) {
  const Icon = item.icon

  return (
    <motion.a
      className="centered-navigation__card"
      href={item.href}
      data-tone={item.tone ?? "neutral"}
      initial={reduceMotion ? false : { opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{
        duration: reduceMotion ? 0.01 : 0.2,
        delay: reduceMotion ? 0 : index * 0.045,
        ease: [0.22, 1, 0.36, 1],
      }}
    >
      <span className="centered-navigation__card-icon">
        <Icon aria-hidden="true" />
      </span>
      <span className="centered-navigation__card-copy">
        <strong>{item.title}</strong>
        <small>{item.description}</small>
      </span>
    </motion.a>
  )
}

export default CenteredNavigation
