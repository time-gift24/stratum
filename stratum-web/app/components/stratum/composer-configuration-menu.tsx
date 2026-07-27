"use client"

import { IconAdjustmentsHorizontal } from "@tabler/icons-react"
import { AnimatePresence, motion, useReducedMotion } from "motion/react"
import type { ReactNode } from "react"
import { useTranslation } from "react-i18next"

import {
  FeatureCard,
  FeatureCardContent,
} from "~/components/stratum/feature-card"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "~/components/ui/dropdown-menu"
import type { ComposerConfiguration } from "~/hooks/use-agent-conversation"
import { modelDisplayName, supportsThinkingControls } from "~/lib/model-config"
import { cn } from "~/lib/utils"

type ComposerConfigurationMenuProps = {
  configuration: ComposerConfiguration
  commandPending: boolean
}

const CONTROL_WELL_CLASS =
  "min-h-11 rounded-lg bg-background/92 text-foreground shadow-[inset_0_1px_0_color-mix(in_srgb,var(--foreground)_5%,transparent),0_10px_24px_color-mix(in_srgb,var(--background)_44%,transparent)]"

const RADIO_ITEM_CLASS =
  "min-h-10 rounded-lg px-2.5 text-sm transition-colors data-checked:bg-foreground/[0.065] data-checked:text-foreground"

function ConfigurationRow({
  label,
  signalClassName,
  children,
}: {
  label: string
  signalClassName: string
  children: ReactNode
}) {
  return (
    <div className="grid grid-cols-[6.25rem_minmax(0,1fr)] items-center gap-3">
      <span className="flex items-center gap-2 text-sm text-muted-foreground">
        <span
          aria-hidden="true"
          className={cn("size-2 rounded-full shadow-md", signalClassName)}
        />
        {label}
      </span>
      {children}
    </div>
  )
}

export function ComposerConfigurationMenu({
  configuration,
  commandPending,
}: ComposerConfigurationMenuProps) {
  const { t } = useTranslation()
  const reduceMotion = useReducedMotion()
  const selected = configuration.selectedModelConfig
  const selectedDescriptor = configuration.models.find(
    (descriptor) => descriptor.model === selected?.model
  )
  const thinking =
    selected === null ? "disabled" : thinkingLevel(selected.parameters)
  const thinkingAvailable =
    selected !== null &&
    selectedDescriptor !== undefined &&
    supportsThinkingControls(selectedDescriptor.parameters_schema)
  const selectedAgentName =
    configuration.selectedTemplate?.agent_name ??
    configuration.agentName ??
    t("chat.composer.selectAgent")
  const selectedModelName = selected
    ? modelDisplayName(selected.model).model
    : t("chat.composer.model")
  const controlsDisabled = menuDisabled(configuration, commandPending)

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        aria-label={t("chat.composer.configure")}
        className="group/config grid size-10 shrink-0 place-items-center rounded-lg bg-sidebar-accent/58 text-sidebar-foreground/88 shadow-lg shadow-background/15 outline-hidden backdrop-blur-xl transition-[background-color,color,box-shadow,transform] duration-200 hover:-translate-y-0.5 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:ring-2 focus-visible:ring-sidebar-ring disabled:pointer-events-none disabled:opacity-45 data-popup-open:bg-chart-1/14 data-popup-open:text-chart-1 data-popup-open:shadow-chart-1/5 motion-reduce:transition-none"
      >
        <IconAdjustmentsHorizontal
          className="size-[1.125rem] transition-transform duration-200 group-hover/config:rotate-6 group-data-popup-open/config:rotate-90 motion-reduce:transition-none"
          strokeWidth={2.4}
          aria-hidden="true"
        />
      </DropdownMenuTrigger>

      <DropdownMenuContent
        align="end"
        side="top"
        sideOffset={12}
        className="max-h-none w-[min(28rem,calc(100vw-2rem))] overflow-visible border-0 bg-transparent p-0 shadow-none ring-0"
      >
        <FeatureCard className="p-2">
          <header className="flex h-9 items-center gap-2 px-2">
            <span
              aria-hidden="true"
              className="size-2 rounded-full bg-foreground shadow-[0_0_10px_color-mix(in_srgb,var(--foreground)_50%,transparent)]"
            />
            <h2 className="font-heading text-sm font-semibold text-foreground">
              {t("chat.composer.configuration")}
            </h2>
          </header>

          <FeatureCardContent className="max-h-[min(34rem,calc(100dvh-8rem))] overflow-y-auto p-4">
            {configuration.metadataLoading ? (
              <div className="grid min-h-20 place-items-center px-4 text-center text-sm text-muted-foreground">
                {t("chat.composer.loadingConfiguration")}
              </div>
            ) : configuration.metadataError !== null ? (
              <div className="grid min-h-20 place-items-center px-4 text-center text-sm text-destructive">
                {t("chat.composer.configurationUnavailable", {
                  defaultValue: t("chat.connectionFailed"),
                })}
              </div>
            ) : (
              <div className="grid gap-3">
                <ConfigurationRow
                  label={t("chat.composer.agent")}
                  signalClassName="bg-primary shadow-primary/35"
                >
                  <DropdownMenuSub>
                    <DropdownMenuSubTrigger
                      disabled={controlsDisabled}
                      className={cn(
                        CONTROL_WELL_CLASS,
                        "w-full px-3 text-sm font-medium [&>svg]:text-muted-foreground"
                      )}
                    >
                      <span className="truncate">{selectedAgentName}</span>
                    </DropdownMenuSubTrigger>
                    <DropdownMenuSubContent className="w-64 border-0 bg-popover/92 p-1 shadow-xl ring-0 backdrop-blur-xl">
                      <DropdownMenuRadioGroup
                        value={
                          configuration.selectedTemplate?.agent_name ??
                          configuration.agentName ??
                          undefined
                        }
                        onValueChange={(agentName) => {
                          const template = configuration.agentTemplates.find(
                            (candidate) => candidate.agent_name === agentName
                          )
                          if (template) configuration.selectTemplate(template)
                        }}
                      >
                        {configuration.agentTemplates.map((template) => (
                          <DropdownMenuRadioItem
                            key={template.agent_name}
                            value={template.agent_name}
                            closeOnClick={false}
                            className={RADIO_ITEM_CLASS}
                          >
                            <span className="truncate">
                              {template.agent_name}
                            </span>
                          </DropdownMenuRadioItem>
                        ))}
                      </DropdownMenuRadioGroup>
                    </DropdownMenuSubContent>
                  </DropdownMenuSub>
                </ConfigurationRow>

                <ConfigurationRow
                  label={t("chat.composer.model")}
                  signalClassName="bg-chart-1 shadow-chart-1/35"
                >
                  <DropdownMenuSub>
                    <DropdownMenuSubTrigger
                      disabled={controlsDisabled}
                      className={cn(
                        CONTROL_WELL_CLASS,
                        "w-full px-3 text-sm font-medium [&>svg]:text-muted-foreground"
                      )}
                    >
                      <span className="truncate">{selectedModelName}</span>
                    </DropdownMenuSubTrigger>
                    <DropdownMenuSubContent className="w-72 border-0 bg-popover/92 p-1 shadow-xl ring-0 backdrop-blur-xl">
                      <DropdownMenuRadioGroup
                        value={selected?.model}
                        onValueChange={(model) => {
                          const descriptor = configuration.models.find(
                            (candidate) => candidate.model === model
                          )
                          if (descriptor) configuration.selectModel(descriptor)
                        }}
                      >
                        {configuration.models.map((descriptor) => {
                          const displayName = modelDisplayName(descriptor.model)
                          return (
                            <DropdownMenuRadioItem
                              key={descriptor.model}
                              value={descriptor.model}
                              closeOnClick={false}
                              className={RADIO_ITEM_CLASS}
                            >
                              <span className="min-w-0 flex-1 truncate">
                                {displayName.model}
                              </span>
                              {displayName.provider ? (
                                <span className="shrink-0 pr-1 text-xs text-muted-foreground">
                                  {displayName.provider}
                                </span>
                              ) : null}
                            </DropdownMenuRadioItem>
                          )
                        })}
                      </DropdownMenuRadioGroup>
                    </DropdownMenuSubContent>
                  </DropdownMenuSub>
                </ConfigurationRow>

                <AnimatePresence initial={false}>
                  {thinkingAvailable ? (
                    <motion.div
                      key="thinking-controls"
                      initial={{ height: 0, opacity: 0 }}
                      animate={{ height: "auto", opacity: 1 }}
                      exit={{ height: 0, opacity: 0 }}
                      transition={{
                        duration: reduceMotion ? 0 : 0.18,
                        ease: [0.22, 1, 0.36, 1],
                      }}
                      className="overflow-hidden"
                    >
                      <ConfigurationRow
                        label={t("chat.composer.thinking")}
                        signalClassName="bg-chart-2 shadow-chart-2/35"
                      >
                        <DropdownMenuRadioGroup
                          value={thinking}
                          onValueChange={(value) => {
                            if (
                              value === "disabled" ||
                              value === "high" ||
                              value === "max"
                            )
                              configuration.setThinkingLevel(value)
                          }}
                          className="grid grid-cols-3 gap-1"
                        >
                          {(["disabled", "high", "max"] as const).map(
                            (level) => (
                              <DropdownMenuRadioItem
                                key={level}
                                value={level}
                                closeOnClick={false}
                                disabled={controlsDisabled}
                                className={cn(
                                  CONTROL_WELL_CLASS,
                                  "min-h-11 justify-center px-2 pr-2 text-xs data-checked:bg-chart-2/12 data-checked:text-chart-2 [&_[data-slot=dropdown-menu-radio-item-indicator]]:hidden"
                                )}
                              >
                                {t(`chat.composer.${level}`)}
                              </DropdownMenuRadioItem>
                            )
                          )}
                        </DropdownMenuRadioGroup>
                      </ConfigurationRow>
                    </motion.div>
                  ) : null}
                </AnimatePresence>
              </div>
            )}
          </FeatureCardContent>
        </FeatureCard>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function menuDisabled(
  configuration: ComposerConfiguration,
  commandPending: boolean
): boolean {
  return (
    configuration.metadataLoading ||
    configuration.metadataError !== null ||
    configuration.turnRunning ||
    commandPending
  )
}

function thinkingLevel(
  parameters: Record<string, unknown>
): "disabled" | "high" | "max" {
  const thinking = parameters.thinking
  if (typeof thinking !== "object" || thinking === null) return "disabled"

  const level = thinking as Record<string, unknown>
  return level.reasoning_effort === "max"
    ? "max"
    : level.reasoning_effort === "high"
      ? "high"
      : "disabled"
}
