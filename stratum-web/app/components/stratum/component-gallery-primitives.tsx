import {
  CircleAlert,
  Component,
  Navigation,
  SlidersHorizontal,
} from "lucide-react"

import { Badge } from "~/components/ui/badge"
import { Button } from "~/components/ui/button"
import { Input } from "~/components/ui/input"

export type SpectrumItem = {
  label: string
  value: string
  className: string
}

export function PaletteSpectrum({
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

export function SectionCopy({
  title,
  description,
}: {
  title: string
  description: string
}) {
  return (
    <div className="max-w-lg [&>h2]:font-heading [&>h2]:text-3xl [&>h2]:font-medium [&>h2]:tracking-tight [&>p]:mt-3 [&>p]:max-w-md [&>p]:text-base [&>p]:leading-7 [&>p]:text-muted-foreground">
      <h2>{title}</h2>
      <p>{description}</p>
    </div>
  )
}

export function NavigationCalibration() {
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

export function SpecificationList({
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

export function ControlLedger({
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

export function StateLine({
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
