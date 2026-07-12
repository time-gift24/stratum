"use client"

import { useState } from "react"
import { HistoryIcon } from "lucide-react"
import { useTranslation } from "react-i18next"

import { ChatWorkspace } from "~/components/stratum/chat-workspace"
import { cn } from "~/lib/utils"
import { RouteTransition } from "~/components/stratum/route-transition"
import { SiteNavbar } from "~/components/stratum/site-navbar"
import { Button } from "~/components/ui/button"

export default function Longzhong() {
  const { t } = useTranslation()
  const [historyOpen, setHistoryOpen] = useState(false)

  return (
    <RouteTransition>
      <main>
        <SiteNavbar
          activeSection="longzhong"
          leftSlot={
            <Button
              variant="ghost"
              size="icon-lg"
              aria-label={t("chat.history.title")}
              aria-expanded={historyOpen}
              aria-controls="chat-history-drawer"
              onClick={() => setHistoryOpen((open) => !open)}
              className={cn(
                "rounded-full bg-wyse-paper shadow-wyse-soft ring-1 ring-wyse-line",
                "hover:bg-wyse-paper-wash",
                historyOpen
                  ? "text-wyse-action ring-wyse-action/30"
                  : "text-muted-foreground hover:text-foreground"
              )}
            >
              <HistoryIcon className="size-5" aria-hidden="true" />
            </Button>
          }
        />
        <ChatWorkspace
          historyOpen={historyOpen}
          onHistoryOpenChange={setHistoryOpen}
        />
      </main>
    </RouteTransition>
  )
}
