import {
  IconBrain,
  IconChevronDown,
  IconCpu,
  IconRobot,
} from "@tabler/icons-react"
import { useTranslation } from "react-i18next"

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "~/components/ui/dropdown-menu"
import type { ComposerConfiguration } from "~/hooks/use-agent-conversation"
import { modelDisplayName, supportsThinkingControls } from "~/lib/model-config"

type ConfigurationMenuProps = {
  configuration: ComposerConfiguration
  commandPending: boolean
}

const COMPOSER_TOOL_CLASS =
  "inline-flex h-10 min-w-0 flex-1 items-center gap-2 rounded-lg border border-transparent bg-transparent px-2.5 text-xs font-medium text-muted-foreground transition-[background-color,border-color,color,box-shadow] duration-200 outline-none hover:bg-secondary hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50 data-popup-open:bg-secondary data-popup-open:text-foreground data-popup-open:shadow-sm data-[state=open]:bg-secondary data-[state=open]:text-foreground sm:flex-none"

export function AgentConfigMenu({
  configuration,
  commandPending,
}: ConfigurationMenuProps) {
  const { t } = useTranslation()
  const triggerText = configuration.metadataLoading
    ? t("chat.composer.loadingConfiguration")
    : (configuration.agentName ?? t("chat.composer.selectAgent"))

  if (configuration.agentTemplates.length <= 1) return null

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        aria-label={triggerText}
        data-tone="agent"
        className={`${COMPOSER_TOOL_CLASS} max-w-40`}
        disabled={menuDisabled(configuration, commandPending)}
      >
        <IconRobot className="size-3.5 shrink-0" aria-hidden="true" />
        <span className="truncate">{triggerText}</span>
        <IconChevronDown className="size-3.5 shrink-0" aria-hidden="true" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-56">
        <DropdownMenuGroup>
          <DropdownMenuLabel className="text-sm font-semibold">
            {t("chat.composer.agent")}
          </DropdownMenuLabel>
          <DropdownMenuRadioGroup
            value={configuration.agentName ?? undefined}
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
                className="min-h-11 text-sm"
              >
                {template.agent_name}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

export function ModelConfigMenu({
  configuration,
  commandPending,
}: ConfigurationMenuProps) {
  const { t } = useTranslation()
  const selected = configuration.selectedModelConfig
  const selectedDescriptor = configuration.models.find(
    (descriptor) => descriptor.model === selected?.model
  )
  const modelGroups = new Map<
    string,
    (typeof configuration.models)[number][]
  >()
  for (const descriptor of configuration.models) {
    const displayName = modelDisplayName(descriptor.model)
    const provider = displayName.provider ?? t("chat.composer.model")
    const models = modelGroups.get(provider)
    if (models) models.push(descriptor)
    else modelGroups.set(provider, [descriptor])
  }
  const triggerText = configuration.metadataLoading
    ? t("chat.composer.loadingConfiguration")
    : selected === null
      ? t("chat.composer.selectAgent")
      : modelDisplayName(selected.model).model
  const selectedThinkingLevel =
    selected === null ? "disabled" : thinkingLevel(selected.parameters)
  const thinkingText =
    selectedThinkingLevel === "max"
      ? t("chat.composer.max")
      : selectedThinkingLevel === "high"
        ? t("chat.composer.high")
        : t("chat.composer.disabled")
  const thinkingAvailable =
    selected !== null &&
    selectedDescriptor !== undefined &&
    supportsThinkingControls(selectedDescriptor.parameters_schema)

  return (
    <DropdownMenu>
        <DropdownMenuTrigger
          aria-label={`${triggerText}, ${t("chat.composer.thinking")}: ${thinkingText}`}
          data-tone="model"
          className={`${COMPOSER_TOOL_CLASS} max-w-56 data-popup-open:bg-chart-1/16 data-popup-open:text-chart-1 data-popup-open:ring-1 data-popup-open:ring-chart-1/30 data-popup-open:shadow-[0_8px_24px_color-mix(in_srgb,var(--chart-1)_12%,transparent)]`}
          disabled={
            configuration.currentModelConfig === null ||
            menuDisabled(configuration, commandPending)
          }
        >
          <IconCpu className="size-3.5 shrink-0" aria-hidden="true" />
          <span className="truncate font-medium">
            {triggerText}
            {thinkingAvailable ? (
              <span className="text-muted-foreground"> · {thinkingText}</span>
            ) : null}
          </span>
          <IconChevronDown
            className="size-3.5 shrink-0 opacity-65"
            aria-hidden="true"
          />
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-64">
          <DropdownMenuGroup>
            <DropdownMenuLabel className="text-sm font-semibold">
              {t("chat.composer.model")}
            </DropdownMenuLabel>
            {Array.from(modelGroups.entries()).map(
              ([provider, descriptors]) => (
                <DropdownMenuSub key={provider}>
                  <DropdownMenuSubTrigger className="min-h-11 text-sm font-medium">
                    {provider}
                  </DropdownMenuSubTrigger>
                  <DropdownMenuSubContent className="w-64">
                    <DropdownMenuRadioGroup
                      value={selected?.model}
                      onValueChange={(model) => {
                        const descriptor = configuration.models.find(
                          (candidate) => candidate.model === model
                        )
                        if (descriptor) configuration.selectModel(descriptor)
                      }}
                    >
                      {descriptors.map((descriptor) => (
                        <DropdownMenuRadioItem
                          key={descriptor.model}
                          value={descriptor.model}
                          className="min-h-11 text-sm"
                        >
                          {modelDisplayName(descriptor.model).model}
                        </DropdownMenuRadioItem>
                      ))}
                    </DropdownMenuRadioGroup>
                  </DropdownMenuSubContent>
                </DropdownMenuSub>
              )
            )}
            {thinkingAvailable ? (
              <DropdownMenuSub>
                <DropdownMenuSubTrigger className="mt-1 min-h-11 text-sm font-medium">
                  <IconBrain className="text-chart-2" aria-hidden="true" />
                  <span className="truncate">
                    {t("chat.composer.thinking")} · {thinkingText}
                  </span>
                </DropdownMenuSubTrigger>
                <DropdownMenuSubContent className="w-44">
                  <DropdownMenuRadioGroup
                    value={selectedThinkingLevel}
                    onValueChange={(value) => {
                      if (
                        value === "disabled" ||
                        value === "high" ||
                        value === "max"
                      )
                        configuration.setThinkingLevel(value)
                    }}
                  >
                    <DropdownMenuRadioItem
                      value="disabled"
                      className="min-h-11 text-sm"
                    >
                      {t("chat.composer.disabled")}
                    </DropdownMenuRadioItem>
                    <DropdownMenuRadioItem
                      value="high"
                      className="min-h-11 text-sm"
                    >
                      {t("chat.composer.high")}
                    </DropdownMenuRadioItem>
                    <DropdownMenuRadioItem
                      value="max"
                      className="min-h-11 text-sm"
                    >
                      {t("chat.composer.max")}
                    </DropdownMenuRadioItem>
                  </DropdownMenuRadioGroup>
                </DropdownMenuSubContent>
              </DropdownMenuSub>
            ) : null}
          </DropdownMenuGroup>
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
    commandPending ||
    (configuration.existingAgent && configuration.currentModelConfig === null)
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
