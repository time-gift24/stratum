import {
  Bell,
  Brackets,
  Component,
  History,
  Navigation,
  SlidersHorizontal,
  SquarePen,
  type LucideIcon,
} from "lucide-react"

import type { VerticalNavigationTone } from "~/components/stratum/vertical-navigation"

export type NavigationDefinition = {
  id: string
  icon: LucideIcon
  labelKey: string
  href?: string
  action?: "open-history"
  tone?: VerticalNavigationTone
}

export const CHAT_NAVIGATION_DEFINITIONS = [
  {
    id: "new-conversation",
    icon: SquarePen,
    labelKey: "productShell.newConversation",
    href: "/chat?new=1",
    tone: "green",
  },
  {
    id: "history",
    icon: History,
    labelKey: "productShell.recent",
    action: "open-history",
    tone: "blue",
  },
] as const satisfies readonly NavigationDefinition[]

export const COMPONENT_GALLERY_NAVIGATION_DEFINITIONS = [
  {
    id: "foundations",
    icon: Brackets,
    labelKey: "componentGallery.navigation.foundations",
    href: "#foundations",
    tone: "blue",
  },
  {
    id: "navigation",
    icon: Navigation,
    labelKey: "componentGallery.navigation.navigation",
    href: "#navigation",
    tone: "yellow",
  },
  {
    id: "controls",
    icon: SlidersHorizontal,
    labelKey: "componentGallery.navigation.controls",
    href: "#controls",
    tone: "magenta",
  },
  {
    id: "states",
    icon: Bell,
    labelKey: "componentGallery.navigation.states",
    href: "#states",
    tone: "neutral",
  },
] as const satisfies readonly NavigationDefinition[]

export const GLOBAL_DEVELOPMENT_NAVIGATION_DEFINITIONS = [
  {
    id: "component-gallery",
    icon: Component,
    titleKey: "globalNavigation.components",
    descriptionKey: "globalNavigation.componentsDescription",
    href: "/component-gallery",
    tone: "yellow",
  },
] as const
