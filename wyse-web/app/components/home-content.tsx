"use client"

import { ArrowRightIcon } from "lucide-react"

import { useLocale } from "~/components/locale-provider"
import { SiteNavbar } from "~/components/site-navbar"
import { Button } from "~/components/ui/button"
import { ChatWorkspace } from "~/components/chat-workspace"
import { useWorkspacePager, WorkspacePager } from "~/components/workspace-pager"
import { OrchestrationWorkspace } from "~/components/orchestration-workspace"

export function HomeContent() {
  return (
    <WorkspacePager>
      <BrandIntro />
      <ChatWorkspace />
      <OrchestrationWorkspace />
    </WorkspacePager>
  )
}

function BrandIntro() {
  const { t } = useLocale()
  const { selectSlide } = useWorkspacePager()

  return (
    <section data-workspace-slide="intro" className="wyse-intro-slide">
      <SiteNavbar />
      <div className="wyse-intro-slide__content">
        <p className="wyse-intro-slide__brand">运筹 / Stratum</p>
        <div className="wyse-intro-slide__copy">
          <h1 className="wyse-intro-slide__title">{t("hero.title")}</h1>
          <p className="wyse-intro-slide__body">{t("hero.body")}</p>
        </div>
        <Button onClick={() => selectSlide(1)} size="lg" type="button">
          {t("hero.enter")}
          <ArrowRightIcon data-icon="inline-end" aria-hidden="true" />
        </Button>
      </div>
    </section>
  )
}
