"use client"

import { Component } from "lucide-react"
import { useLocation } from "react-router"
import { useTranslation } from "react-i18next"

import {
  CenteredNavigation,
  type CenteredNavigationGroup,
  type CenteredNavigationLink,
} from "~/components/react-bits/centered-navigation"
import { LanguageToggle } from "~/components/stratum/language-toggle"

export function GlobalNavigation() {
  const { t } = useTranslation()
  const location = useLocation()
  const chatHref =
    location.pathname === "/chat"
      ? `${location.pathname}${location.search}`
      : "/chat"
  const links: readonly CenteredNavigationLink[] = [
    {
      label: t("globalNavigation.chat"),
      href: chatHref,
    },
  ]
  const groups: readonly CenteredNavigationGroup[] = import.meta.env.DEV
    ? [
        {
          id: "development",
          label: t("globalNavigation.development"),
          items: [
            {
              icon: Component,
              title: t("globalNavigation.components"),
              description: t("globalNavigation.componentsDescription"),
              href: "/component-gallery",
              tone: "yellow",
            },
          ],
        },
      ]
    : []

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
