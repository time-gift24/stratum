/**
 * THESIS: A component calibration bench, not a generic Storybook card wall.
 * OWN-WORLD: Graphite black, white type, precise hairlines, and restrained signal colors.
 * STORY: Review the system foundations, operate the navigation, then compare control states.
 */
"use client"

import {
  Bell,
  Brackets,
  CircleAlert,
  Component,
  Navigation,
  SlidersHorizontal,
} from "lucide-react"
import { useTranslation } from "react-i18next"

import {
  VerticalNavigation,
  type VerticalNavigationItem,
} from "~/components/stratum/vertical-navigation"
import { Badge } from "~/components/ui/badge"
import { Button } from "~/components/ui/button"
import { Input } from "~/components/ui/input"

const SECTION_CLASS =
  "grid scroll-mt-28 gap-10 py-16 lg:grid-cols-[minmax(0,0.72fr)_minmax(0,1.28fr)] lg:items-start"
const SECTION_COPY_CLASS =
  "max-w-lg [&>h2]:font-heading [&>h2]:text-3xl [&>h2]:font-medium [&>h2]:tracking-tight [&>p]:mt-3 [&>p]:max-w-md [&>p]:text-base [&>p]:leading-7 [&>p]:text-muted-foreground"

type SpectrumItem = {
  label: string
  value: string
  className: string
}

function PaletteSpectrum({
  label,
  items,
}: {
  label: string
  items: readonly SpectrumItem[]
}) {
  return (
    <div
      className="grid overflow-hidden rounded-xl shadow-2xl sm:grid-cols-2 lg:grid-cols-4"
      role="group"
      aria-label={label}
    >
      {items.map((item) => (
        <div
          key={item.value}
          className={`flex min-h-32 flex-col justify-between p-5 text-background ${item.className}`}
        >
          <span className="text-sm font-semibold">{item.label}</span>
          <code className="font-mono text-xs font-medium">{item.value}</code>
        </div>
      ))}
    </div>
  )
}

function SectionCopy({
  title,
  description,
}: {
  title: string
  description: string
}) {
  return (
    <div className={SECTION_COPY_CLASS}>
      <h2>{title}</h2>
      <p>{description}</p>
    </div>
  )
}

function NavigationCalibration() {
  return (
    <div
      className="relative flex min-h-52 items-center justify-center gap-4 overflow-hidden rounded-xl bg-card p-8 shadow-xl"
      aria-hidden="true"
    >
      <div className="absolute inset-x-8 top-1/2 h-px bg-border/65" />
      <div className="relative grid size-12 place-items-center rounded-lg bg-secondary text-muted-foreground shadow-lg [&>svg]:size-5">
        <Component />
      </div>
      <div className="relative grid size-15 place-items-center rounded-lg bg-chart-2/14 text-chart-2 shadow-xl [&>svg]:size-6">
        <Navigation />
      </div>
      <div className="relative grid size-12 place-items-center rounded-lg bg-secondary text-muted-foreground shadow-lg [&>svg]:size-5">
        <SlidersHorizontal />
      </div>
    </div>
  )
}

function SpecificationList({
  items,
}: {
  items: readonly { label: string; value: string }[]
}) {
  return (
    <dl className="mt-4 grid gap-px overflow-hidden rounded-xl bg-border/60 sm:grid-cols-3">
      {items.map((item) => (
        <div key={item.label} className="bg-card p-4">
          <dt className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
            {item.label}
          </dt>
          <dd className="mt-2 font-mono text-sm text-foreground">
            {item.value}
          </dd>
        </div>
      ))}
    </dl>
  )
}

function ControlLedger({
  actionsLabel,
  inputLabel,
  primaryLabel,
  secondaryLabel,
  outlineLabel,
  disabledLabel,
  placeholder,
}: {
  actionsLabel: string
  inputLabel: string
  primaryLabel: string
  secondaryLabel: string
  outlineLabel: string
  disabledLabel: string
  placeholder: string
}) {
  return (
    <div className="overflow-hidden rounded-xl bg-card shadow-xl">
      <div className="grid gap-4 p-5 sm:grid-cols-[8rem_1fr] sm:items-start">
        <span className="pt-2 text-sm font-medium text-muted-foreground">
          {actionsLabel}
        </span>
        <div className="flex flex-wrap gap-2">
          <Button>{primaryLabel}</Button>
          <Button variant="secondary">{secondaryLabel}</Button>
          <Button variant="outline">{outlineLabel}</Button>
          <Button disabled>{disabledLabel}</Button>
        </div>
      </div>
      <div className="grid gap-4 border-t border-border/55 p-5 sm:grid-cols-[8rem_1fr] sm:items-start">
        <label
          className="pt-2 text-sm font-medium text-muted-foreground"
          htmlFor="component-gallery-input"
        >
          {inputLabel}
        </label>
        <Input
          id="component-gallery-input"
          className="max-w-md"
          placeholder={placeholder}
        />
      </div>
    </div>
  )
}

function StateLine({
  action,
  neutral,
  information,
  selection,
  collaboration,
  error,
}: {
  action: string
  neutral: string
  information: string
  selection: string
  collaboration: string
  error: string
}) {
  return (
    <div className="flex min-h-32 flex-wrap items-center gap-3 rounded-xl bg-card p-5 shadow-xl">
      <Badge>{action}</Badge>
      <Badge variant="secondary">{neutral}</Badge>
      <Badge className="border-chart-1/45 text-chart-1" variant="outline">
        {information}
      </Badge>
      <Badge className="border-chart-2/45 text-chart-2" variant="outline">
        {selection}
      </Badge>
      <Badge className="border-chart-3/45 text-chart-3" variant="outline">
        {collaboration}
      </Badge>
      <Badge variant="destructive">
        <CircleAlert data-icon="inline-start" />
        {error}
      </Badge>
    </div>
  )
}

export default function ComponentsPage() {
  const { t } = useTranslation()
  const navigationItems: readonly VerticalNavigationItem[] = [
    {
      id: "foundations",
      icon: Brackets,
      label: t("componentGallery.navigation.foundations"),
      href: "#foundations",
      tone: "blue",
    },
    {
      id: "navigation",
      icon: Navigation,
      label: t("componentGallery.navigation.navigation"),
      href: "#navigation",
      tone: "yellow",
    },
    {
      id: "controls",
      icon: SlidersHorizontal,
      label: t("componentGallery.navigation.controls"),
      href: "#controls",
      tone: "magenta",
    },
    {
      id: "states",
      icon: Bell,
      label: t("componentGallery.navigation.states"),
      href: "#states",
      tone: "neutral",
    },
  ]
  const spectrumItems: readonly SpectrumItem[] = [
    {
      label: t("componentGallery.palette.information"),
      value: "#6DB5FF",
      className: "bg-chart-1",
    },
    {
      label: t("componentGallery.palette.selection"),
      value: "#FEFA3D",
      className: "bg-chart-2",
    },
    {
      label: t("componentGallery.palette.collaboration"),
      value: "#FF5DE7",
      className: "bg-chart-3",
    },
    {
      label: t("componentGallery.palette.action"),
      value: "#78ED9D",
      className: "bg-chart-4",
    },
  ]

  return (
    <div className="min-h-[calc(100dvh-var(--global-nav-offset))] bg-background text-foreground">
      <VerticalNavigation
        items={navigationItems}
        activeId="foundations"
        ariaLabel={t("componentGallery.navigation.label")}
      />
      <main
        className="mx-auto w-full max-w-6xl px-6 pt-12 pb-24 sm:px-10 lg:px-16"
        id="main-content"
      >
        <section className="scroll-mt-28 pt-8 pb-16" id="foundations">
          <div className="mb-12 max-w-4xl">
            <h1 className="font-heading text-5xl leading-none font-medium tracking-[-0.035em] sm:text-7xl lg:text-8xl">
              {t("componentGallery.title")}
            </h1>
            <p className="mt-6 max-w-2xl text-base leading-7 text-muted-foreground sm:text-lg">
              {t("componentGallery.description")}
            </p>
          </div>
          <PaletteSpectrum
            label={t("componentGallery.palette.label")}
            items={spectrumItems}
          />
        </section>

        <section className={SECTION_CLASS} id="navigation">
          <SectionCopy
            title={t("componentGallery.verticalNavigation.title")}
            description={t("componentGallery.verticalNavigation.description")}
          />
          <div>
            <NavigationCalibration />
            <SpecificationList
              items={[
                {
                  label: t("componentGallery.verticalNavigation.desktop"),
                  value: "48–60 px",
                },
                {
                  label: t("componentGallery.verticalNavigation.mobile"),
                  value: "44 px",
                },
                {
                  label: t("componentGallery.verticalNavigation.motion"),
                  value: "180 / 16",
                },
              ]}
            />
          </div>
        </section>

        <section className={SECTION_CLASS} id="controls">
          <SectionCopy
            title={t("componentGallery.controls.title")}
            description={t("componentGallery.controls.description")}
          />
          <ControlLedger
            actionsLabel={t("componentGallery.controls.actions")}
            inputLabel={t("componentGallery.controls.input")}
            primaryLabel={t("componentGallery.controls.primary")}
            secondaryLabel={t("componentGallery.controls.secondary")}
            outlineLabel={t("componentGallery.controls.outline")}
            disabledLabel={t("componentGallery.controls.disabled")}
            placeholder={t("componentGallery.controls.placeholder")}
          />
        </section>

        <section className={SECTION_CLASS} id="states">
          <SectionCopy
            title={t("componentGallery.states.title")}
            description={t("componentGallery.states.description")}
          />
          <StateLine
            action={t("componentGallery.states.action")}
            neutral={t("componentGallery.states.neutral")}
            information={t("componentGallery.states.information")}
            selection={t("componentGallery.states.selection")}
            collaboration={t("componentGallery.states.collaboration")}
            error={t("componentGallery.states.error")}
          />
        </section>
      </main>
    </div>
  )
}
