"use client"

import { useId, useState, type ReactNode } from "react"
import { ChevronDown, ChevronLeft, ChevronRight, Send } from "lucide-react"
import { useTranslation } from "react-i18next"

import {
  FeatureCard,
  FeatureCardContent,
} from "~/components/stratum/feature-card"
import { Button } from "~/components/ui/button"
import { cn } from "~/lib/utils"

const CONTROL_WELL_CLASS =
  "min-h-11 rounded-lg bg-background/92 text-foreground shadow-[inset_0_1px_0_color-mix(in_srgb,var(--foreground)_5%,transparent),0_10px_24px_color-mix(in_srgb,var(--background)_44%,transparent)]"

function SignalLabel({
  className,
  children,
}: {
  className: string
  children: ReactNode
}) {
  return (
    <span className="flex items-center gap-2 text-sm text-muted-foreground">
      <span
        aria-hidden="true"
        className={cn("size-2 rounded-full shadow-md", className)}
      />
      {children}
    </span>
  )
}

function ParameterRow({
  label,
  labelFor,
  children,
}: {
  label: string
  labelFor?: string
  children: ReactNode
}) {
  return (
    <div className="grid grid-cols-[minmax(6.75rem,0.8fr)_minmax(0,1.2fr)] items-center gap-3">
      {labelFor ? (
        <label className="text-sm text-muted-foreground" htmlFor={labelFor}>
          {label}
        </label>
      ) : (
        <span className="text-sm text-muted-foreground">{label}</span>
      )}
      {children}
    </div>
  )
}

export function FeatureParameterCard({ className }: { className?: string }) {
  const { t } = useTranslation()
  const modeId = useId()
  const strengthId = useId()
  const strategyId = useId()
  const [steps, setSteps] = useState(30)

  return (
    <article className={cn("w-full max-w-md", className)}>
      <FeatureCard>
        <header className="flex min-h-11 items-center gap-3 px-3 pb-2">
          <span
            aria-hidden="true"
            className="size-2.5 rounded-full bg-foreground shadow-[0_0_12px_color-mix(in_srgb,var(--foreground)_54%,transparent)]"
          />
          <h3 className="text-base font-semibold text-foreground">
            {t("componentGallery.controls.featureCard.title")}
          </h3>
        </header>

        <FeatureCardContent className="p-5">
          <div className="flex items-start justify-between gap-5">
            <div className="grid gap-2">
              <SignalLabel className="bg-chart-2 shadow-chart-2/35">
                {t("componentGallery.controls.featureCard.model")}
              </SignalLabel>
              <SignalLabel className="bg-primary shadow-primary/35">
                {t("componentGallery.controls.featureCard.instruction")}
              </SignalLabel>
              <SignalLabel className="bg-destructive shadow-destructive/35">
                {t("componentGallery.controls.featureCard.guardrail")}
              </SignalLabel>
            </div>
            <SignalLabel className="bg-chart-1 shadow-chart-1/35">
              {t("componentGallery.controls.featureCard.runtime")}
            </SignalLabel>
          </div>

          <div className="mt-10 grid gap-3">
            <ParameterRow
              label={t("componentGallery.controls.featureCard.temperature")}
            >
              <div
                className={cn(
                  CONTROL_WELL_CLASS,
                  "flex items-center justify-between px-3"
                )}
              >
                <span className="font-mono text-sm text-muted-foreground">
                  {t("componentGallery.controls.featureCard.temperatureValue")}
                </span>
                <Send
                  aria-hidden="true"
                  className="size-5 rotate-[-18deg] fill-chart-1 text-chart-1"
                />
              </div>
            </ParameterRow>

            <ParameterRow
              label={t("componentGallery.controls.featureCard.mode")}
              labelFor={modeId}
            >
              <div className="relative">
                <select
                  id={modeId}
                  className={cn(
                    CONTROL_WELL_CLASS,
                    "w-full appearance-none px-3 pr-10 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/45"
                  )}
                  defaultValue="fixed"
                >
                  <option value="fixed">
                    {t("componentGallery.controls.featureCard.modeFixed")}
                  </option>
                  <option value="adaptive">
                    {t("componentGallery.controls.featureCard.modeAdaptive")}
                  </option>
                </select>
                <ChevronDown
                  aria-hidden="true"
                  className="pointer-events-none absolute top-1/2 right-3 size-4 -translate-y-1/2 text-foreground"
                />
              </div>
            </ParameterRow>

            <ParameterRow
              label={t("componentGallery.controls.featureCard.steps")}
            >
              <div className="grid grid-cols-[2.75rem_1fr_2.75rem] gap-1.5">
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className={CONTROL_WELL_CLASS}
                  aria-label={t(
                    "componentGallery.controls.featureCard.decrease"
                  )}
                  onClick={() => setSteps((value) => Math.max(1, value - 1))}
                >
                  <ChevronLeft />
                </Button>
                <output
                  className={cn(
                    CONTROL_WELL_CLASS,
                    "grid place-items-center font-mono text-sm text-muted-foreground"
                  )}
                >
                  {steps}
                </output>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className={CONTROL_WELL_CLASS}
                  aria-label={t(
                    "componentGallery.controls.featureCard.increase"
                  )}
                  onClick={() => setSteps((value) => Math.min(99, value + 1))}
                >
                  <ChevronRight />
                </Button>
              </div>
            </ParameterRow>

            <ParameterRow
              label={t("componentGallery.controls.featureCard.strength")}
              labelFor={strengthId}
            >
              <div className="relative">
                <select
                  id={strengthId}
                  className={cn(
                    CONTROL_WELL_CLASS,
                    "w-full appearance-none px-3 pr-10 font-mono text-sm text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring/45"
                  )}
                  defaultValue="8"
                >
                  <option value="6">6.0</option>
                  <option value="8">8.0</option>
                  <option value="10">10.0</option>
                </select>
                <ChevronDown
                  aria-hidden="true"
                  className="pointer-events-none absolute top-1/2 right-3 size-4 -translate-y-1/2 text-foreground"
                />
              </div>
            </ParameterRow>

            <ParameterRow
              label={t("componentGallery.controls.featureCard.strategy")}
              labelFor={strategyId}
            >
              <div className="relative">
                <select
                  id={strategyId}
                  className={cn(
                    CONTROL_WELL_CLASS,
                    "w-full appearance-none px-3 pr-10 text-sm text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring/45"
                  )}
                  defaultValue="balanced"
                >
                  <option value="balanced">
                    {t(
                      "componentGallery.controls.featureCard.strategyBalanced"
                    )}
                  </option>
                  <option value="precise">
                    {t("componentGallery.controls.featureCard.strategyPrecise")}
                  </option>
                  <option value="exploratory">
                    {t(
                      "componentGallery.controls.featureCard.strategyExploratory"
                    )}
                  </option>
                </select>
                <ChevronDown
                  aria-hidden="true"
                  className="pointer-events-none absolute top-1/2 right-3 size-4 -translate-y-1/2 text-foreground"
                />
              </div>
            </ParameterRow>
          </div>
        </FeatureCardContent>
      </FeatureCard>
    </article>
  )
}
