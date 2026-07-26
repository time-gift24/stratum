"use client"

import { useMemo } from "react"
import { useLocation } from "react-router"
import { useTranslation } from "react-i18next"

import {
  CenteredNavigation,
  type CenteredNavigationGroup,
  type CenteredNavigationLink,
} from "~/components/stratum/centered-navigation"
import { LanguageToggle } from "~/components/stratum/language-toggle"
import { GLOBAL_DEVELOPMENT_NAVIGATION_DEFINITIONS } from "~/config/navigation"

export function GlobalNavigation() {
  const { t } = useTranslation()
  const location = useLocation()
  const chatHref =
    location.pathname === "/chat"
      ? `${location.pathname}${location.search}`
      : "/chat"
  const links = useMemo<readonly CenteredNavigationLink[]>(
    () => [
      {
        label: t("globalNavigation.chat"),
        href: chatHref,
      },
    ],
    [chatHref, t]
  )
  const groups = useMemo<readonly CenteredNavigationGroup[]>(
    () =>
      import.meta.env.DEV
        ? [
            {
              id: "development",
              label: t("globalNavigation.development"),
              items: GLOBAL_DEVELOPMENT_NAVIGATION_DEFINITIONS.map((item) => ({
                icon: item.icon,
                title: t(item.titleKey),
                description: t(item.descriptionKey),
                href: item.href,
                tone: item.tone,
              })),
            },
          ]
        : [],
    [t]
  )

  return (
    <CenteredNavigation
      ariaLabel={t("globalNavigation.label")}
      brandHref={chatHref}
      brandLabel="运筹 Stratum"
      groups={groups}
      links={links}
      openMenuLabel={t("globalNavigation.openMenu")}
      closeMenuLabel={t("globalNavigation.closeMenu")}
      utility={<LanguageToggle compact />}
    />
  )
}
