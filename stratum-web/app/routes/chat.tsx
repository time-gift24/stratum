"use client"

import { useEffect, useRef } from "react"
import { History, SquarePen } from "lucide-react"
import { useLocation, useNavigate } from "react-router"
import { useTranslation } from "react-i18next"

import {
  VerticalNavigation,
  type VerticalNavigationItem,
} from "~/components/stratum/vertical-navigation"
import { ChatWorkspace } from "~/components/stratum/chat-workspace"
import { useProductWorkbench } from "~/components/stratum/product-shell"
import { RouteTransition } from "~/components/stratum/route-transition"
import { useAgentConversation } from "~/hooks/use-agent-conversation"

/*
Direction contract
AUDIENCE: Agent OS users who need a calm place to begin or continue real work.
GOAL: Make the composer the sole visual entry point, with history available on demand.
STYLE: Graphite technology surface; global centered navigation above a page-owned rail.
FORM: Seed ee48eb66, structure 7, overridden by the user-pinned global/page navigation architecture.
NEVER: Canvas metaphors, readiness labels, fake telemetry, duplicate create actions, or explanatory microcopy.
*/
export default function Chat() {
  const { t } = useTranslation()
  const location = useLocation()
  const navigate = useNavigate()
  const { openHistory } = useProductWorkbench()
  const conversation = useAgentConversation()
  const { selectAgent, composerConfiguration } = conversation
  const handledSearchRef = useRef<string | null>(null)

  useEffect(() => {
    if (location.search === "") {
      handledSearchRef.current = null
      return
    }
    if (handledSearchRef.current === location.search) return

    const parameters = new URLSearchParams(location.search)
    const agentId = parameters.get("agent")
    const templateName = parameters.get("template")
    const startNew = parameters.get("new") === "1"

    if (startNew) {
      handledSearchRef.current = location.search
      selectAgent(null)
      navigate("/chat", { replace: true })
      return
    }

    if (agentId) {
      handledSearchRef.current = location.search
      selectAgent(agentId)
      return
    }

    if (templateName) {
      if (composerConfiguration.metadataLoading) return
      handledSearchRef.current = location.search
      const template = composerConfiguration.agentTemplates.find(
        (candidate) => candidate.agent_name === templateName
      )
      if (template) composerConfiguration.selectTemplate(template)
      return
    }

    handledSearchRef.current = location.search
  }, [
    composerConfiguration.agentTemplates,
    composerConfiguration.metadataLoading,
    composerConfiguration.selectTemplate,
    location.search,
    navigate,
    selectAgent,
  ])

  const navigationItems: readonly VerticalNavigationItem[] = [
    {
      id: "new-conversation",
      icon: SquarePen,
      label: t("productShell.newConversation"),
      href: "/chat?new=1",
      tone: "green",
    },
    {
      id: "history",
      icon: History,
      label: t("productShell.recent"),
      onSelect: openHistory,
      tone: "blue",
    },
  ]

  return (
    <RouteTransition>
      <div className="relative min-h-[calc(100dvh-var(--global-nav-offset))]">
        <VerticalNavigation
          activeId={
            conversation.state.agentId === null
              ? "new-conversation"
              : "active-conversation"
          }
          ariaLabel={t("globalNavigation.chat")}
          items={navigationItems}
        />
        <ChatWorkspace conversation={conversation} />
      </div>
    </RouteTransition>
  )
}
