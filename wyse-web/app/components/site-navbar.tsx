"use client"

import { useRef } from "react"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"
import { ScrollTrigger } from "gsap/ScrollTrigger"
import { useTranslation } from "react-i18next"

import GlassSurface from "~/components/GlassSurface"
import { LanguageToggle } from "~/components/language-toggle"
import { StratumMark } from "~/components/stratum-mark"
import { ThemeToggle } from "~/components/theme-toggle"
import { Button } from "~/components/ui/button"
import {
  NavigationMenu,
  NavigationMenuItem,
  NavigationMenuLink,
  NavigationMenuList,
  navigationMenuTriggerStyle,
} from "~/components/ui/navigation-menu"
import { Separator } from "~/components/ui/separator"

gsap.registerPlugin(useGSAP, ScrollTrigger)

export function SiteNavbar() {
  const { t } = useTranslation()
  const navRef = useRef<HTMLElement>(null)
  const glassRef = useRef<HTMLDivElement>(null)

  useGSAP(
    () => {
      const glass = glassRef.current

      if (!glass) {
        return
      }

      const reduceMotion = window.matchMedia(
        "(prefers-reduced-motion: reduce)"
      ).matches
      const showGlass = () => {
        gsap.to(glass, {
          autoAlpha: 1,
          scale: 1,
          duration: reduceMotion ? 0 : 0.28,
          ease: "power2.out",
          overwrite: true,
        })
      }
      const hideGlass = () => {
        gsap.to(glass, {
          autoAlpha: 0,
          scale: 0.985,
          duration: reduceMotion ? 0 : 0.2,
          ease: "power2.out",
          overwrite: true,
        })
      }

      let glassVisible = window.scrollY > 12

      const setGlassVisible = (nextVisible: boolean) => {
        if (nextVisible === glassVisible) {
          return
        }

        glassVisible = nextVisible
        if (nextVisible) {
          showGlass()
        } else {
          hideGlass()
        }
      }

      gsap.set(glass, {
        autoAlpha: glassVisible ? 1 : 0,
        scale: glassVisible ? 1 : 0.985,
        transformOrigin: "50% 50%",
      })

      ScrollTrigger.create({
        id: "site-navbar-glass",
        start: 0,
        end: "max",
        onRefresh: () => setGlassVisible(window.scrollY > 12),
        onUpdate: () => setGlassVisible(window.scrollY > 12),
      })
    },
    { scope: navRef }
  )

  return (
    <header
      ref={navRef}
      className="fixed inset-x-0 top-4 z-50 px-4 md:top-6 md:px-8"
    >
      <div className="relative isolate mx-auto flex h-12 w-full max-w-5xl items-center gap-4 px-3">
        <div ref={glassRef} aria-hidden="true" className="site-navbar-glass">
          <GlassSurface
            width="100%"
            height="100%"
            borderRadius={12}
            borderWidth={0.085}
            brightness={58}
            opacity={0.86}
            blur={12}
            displace={0.35}
            backgroundOpacity={0.18}
            saturation={1.35}
            distortionScale={-120}
            redOffset={0}
            greenOffset={10}
            blueOffset={22}
            mixBlendMode="screen"
          />
        </div>

        <a
          href="/"
          className="relative z-10 flex min-w-0 items-center gap-2 text-sm font-medium md:text-base"
          aria-label={`运筹 ${t("brand.home")}`}
        >
          <StratumMark animated={false} variant="compact" className="size-7" />
          <span className="truncate font-heading font-semibold">运筹</span>
        </a>

        <div className="relative z-10 ml-auto flex items-center gap-3">
          <NavigationMenu className="hidden flex-none md:flex">
            <NavigationMenuList>
              <NavigationMenuItem>
                <NavigationMenuLink
                  render={<a href="#runtime" />}
                  className={navigationMenuTriggerStyle()}
                >
                  {t("nav.features")}
                </NavigationMenuLink>
              </NavigationMenuItem>
              <NavigationMenuItem>
                <NavigationMenuLink
                  render={<a href="#workflows" />}
                  className={navigationMenuTriggerStyle()}
                >
                  {t("nav.about")}
                </NavigationMenuLink>
              </NavigationMenuItem>
            </NavigationMenuList>
          </NavigationMenu>
          <Separator orientation="vertical" className="hidden md:block" />
          <LanguageToggle />
          <ThemeToggle />
          <Button size="lg">{t("actions.signUp")}</Button>
        </div>
      </div>
    </header>
  )
}
