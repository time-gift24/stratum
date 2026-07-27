"use client"

import { useMemo } from "react"
import { useTranslation } from "react-i18next"
import { Outlet } from "react-router"

import {
  VerticalNavigation,
  type VerticalNavigationItem,
} from "~/components/stratum/vertical-navigation"
import { COMPONENT_GALLERY_NAVIGATION_DEFINITIONS } from "~/config/navigation"

export default function ComponentGalleryLayout() {
  const { t } = useTranslation()
  const navigationItems = useMemo<readonly VerticalNavigationItem[]>(
    () =>
      COMPONENT_GALLERY_NAVIGATION_DEFINITIONS.map((item) => ({
        id: item.id,
        icon: item.icon,
        label: t(item.labelKey),
        href: item.href,
        tone: item.tone,
      })),
    [t]
  )

  return (
    <div className="min-h-[calc(100dvh-var(--global-nav-offset))] bg-background text-foreground">
      <VerticalNavigation
        items={navigationItems}
        activeId="foundations"
        ariaLabel={t("componentGallery.navigation.label")}
        scrollContainerId="component-gallery-scroll"
      />
      <Outlet />
    </div>
  )
}
