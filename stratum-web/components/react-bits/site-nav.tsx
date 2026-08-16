"use client"

import { useEffect, useRef, useState, useSyncExternalStore } from "react"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"
import { ChevronDown, Menu, X } from "lucide-react"

import { TransitionLink } from "@/components/chrome/page-transition"
import { cn } from "@/lib/utils"

/**
 * SiteNav —— 站点顶部导航（reactbits navigation-2 改造）。
 * 全部内容数据驱动：brand / menus（悬停下拉）/ links（直链）/ cta / actions（右端图标槽）由调用方传入。
 * 颜色只消费最外层 token（card / foreground / muted / border / primary），随主题切换。
 * 品牌语言来自节点世界：主色状态点 + 字标。
 * 吸顶状态机：页顶时全宽展开；滚动超过 12px 收缩为居中浮 pill。
 * 两态同为磨砂质感（bg-card/55 + backdrop-blur-2xl + saturate + hairline + 浅阴影）。
 * 动效全部 GSAP：入场、下拉面板高度展开/收起、菜单项交错、悬停滑动底片。
 * 内部导航链接使用 TransitionLink，页面跳转带方向性转场。
 */

export interface SiteNavMenuItem {
  icon: React.ComponentType<{ className?: string }>
  title: string
  description: string
  href: string
}

export interface SiteNavMenu {
  label: string
  /** 传入后菜单本身也是可点击跳转的链接（悬停仍展开下拉） */
  href?: string
  items: SiteNavMenuItem[]
}

export interface SiteNavProps {
  brand: { name: string; href?: string }
  menus?: SiteNavMenu[]
  links?: { label: string; href: string }[]
  cta?: { label: string; href: string }
  /** 右端图标操作槽（如主题切换、设置入口），桌面与移动面板都会渲染 */
  actions?: React.ReactNode
}

/** 菜单项卡片：桌面下拉（md）与移动面板（sm）共用，尺寸差异走 prop */
function NavMenuItemCard({
  item,
  size = "md",
}: {
  item: SiteNavMenuItem
  size?: "md" | "sm"
}) {
  const Icon = item.icon
  const desktop = size === "md"
  return (
    <TransitionLink
      href={item.href}
      {...(desktop ? { "data-menu-item": true } : {})}
      className={cn(
        "group flex items-start gap-3 border border-border bg-card/20 backdrop-blur-2xl",
        desktop
          ? "rounded-2xl p-4 transition-[border-color,box-shadow] duration-200 hover:border-foreground/20 hover:shadow-md"
          : "rounded-xl p-3 no-underline"
      )}
    >
      <div className="shrink-0 rounded-lg bg-muted p-2">
        <Icon
          className={cn("text-foreground/80", desktop ? "h-5 w-5" : "h-4 w-4")}
        />
      </div>
      <div className="min-w-0 flex-1">
        <h3
          className={cn(
            "mb-0.5 text-sm text-foreground",
            desktop
              ? "font-normal transition-colors group-hover:text-foreground/80"
              : "font-semibold"
          )}
        >
          {item.title}
        </h3>
        <p
          className={cn(
            "text-xs text-muted-foreground",
            desktop && "leading-snug"
          )}
        >
          {item.description}
        </p>
      </div>
    </TransitionLink>
  )
}

export function SiteNav({ brand, menus = [], links = [], cta, actions }: SiteNavProps) {
  const [activeMenu, setActiveMenu] = useState<number | null>(null)
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)
  const rootRef = useRef<HTMLElement>(null)
  const desktopPanelRef = useRef<HTMLDivElement>(null)
  const mobilePanelRef = useRef<HTMLDivElement>(null)
  const prevMenuRef = useRef<number | null>(null)

  // 吸顶状态机：页顶 = 全宽展开、透明无边框；滚动后 = 居中磨砂浮 pill。
  // useSyncExternalStore 订阅 scroll（外部系统），SSR 快照恒为未滚动，无 hydration mismatch。
  const scrolled = useSyncExternalStore(
    (onChange) => {
      window.addEventListener("scroll", onChange, { passive: true })
      return () => window.removeEventListener("scroll", onChange)
    },
    () => window.scrollY > 12,
    () => false
  )

  const { contextSafe } = useGSAP(
    () => {
      // 入场动画尊重 reduced-motion（与 ScrollReveal 同款 gsap.matchMedia）
      const mm = gsap.matchMedia()
      mm.add("(prefers-reduced-motion: no-preference)", () => {
        gsap.from("[data-nav-desktop], [data-nav-mobile]", {
          opacity: 0,
          y: -20,
          duration: 0.5,
          ease: "power2.out",
        })
      })
    },
    { scope: rootRef }
  )

  const prefersReduced = () =>
    window.matchMedia("(prefers-reduced-motion: reduce)").matches

  // 桌面下拉面板：仅在「无 → 有」时播展开动画；菜单间切换不播，避免闪烁
  useEffect(() => {
    const panel = desktopPanelRef.current
    if (
      activeMenu !== null &&
      prevMenuRef.current === null &&
      panel &&
      !prefersReduced()
    ) {
      gsap.fromTo(
        panel,
        { height: 0, opacity: 0 },
        { height: "auto", opacity: 1, duration: 0.3, ease: "power2.out" }
      )
      gsap.fromTo(
        panel.querySelectorAll("[data-menu-item]"),
        { opacity: 0, y: 10 },
        { opacity: 1, y: 0, duration: 0.2, stagger: 0.05, ease: "power2.out" }
      )
    }
    prevMenuRef.current = activeMenu
  }, [activeMenu])

  // 移动面板展开动画
  useEffect(() => {
    const panel = mobilePanelRef.current
    if (mobileMenuOpen && panel) {
      gsap.fromTo(
        panel,
        { height: 0, opacity: 0 },
        { height: "auto", opacity: 1, duration: 0.3, ease: "power2.out" }
      )
    }
  }, [mobileMenuOpen])

  const closeDesktopMenu = () => {
    const panel = desktopPanelRef.current
    if (!panel || prefersReduced()) {
      setActiveMenu(null)
      return
    }
    gsap.to(panel, {
      height: 0,
      opacity: 0,
      duration: 0.2,
      ease: "power2.in",
      onComplete: () => setActiveMenu(null),
    })
  }

  const toggleMobileMenu = () => {
    if (!mobileMenuOpen) {
      const panel = mobilePanelRef.current
      if (panel) {
        // 顶掉进行中的关闭 tween，防止其 onComplete 误关新面板
        gsap.killTweensOf(panel)
        gsap.set(panel, { clearProps: "height,opacity" })
      }
      setMobileMenuOpen(true)
      return
    }
    const panel = mobilePanelRef.current
    if (!panel || prefersReduced()) {
      setMobileMenuOpen(false)
      return
    }
    gsap.to(panel, {
      height: 0,
      opacity: 0,
      duration: 0.2,
      ease: "power2.in",
      onComplete: () => setMobileMenuOpen(false),
    })
  }

  // 悬停滑动底片：跟随当前悬停的菜单/链接，与 dock 同一套物理语言
  const movePill = contextSafe((el: HTMLElement) => {
    const pill = el.parentElement?.querySelector("[data-nav-pill]")
    if (!pill) return
    gsap.to(pill, {
      x: el.offsetLeft,
      width: el.offsetWidth,
      opacity: 1,
      duration: 0.35,
      ease: "power3.out",
    })
  })

  const hidePill = contextSafe((container: HTMLElement) => {
    const pill = container.querySelector("[data-nav-pill]")
    if (!pill) return
    gsap.to(pill, { opacity: 0, duration: 0.2, ease: "power2.out" })
  })

  const brandContent = (
    <>
      <span aria-hidden className="size-2 shrink-0 rounded-full bg-primary" />
      {brand.name}
    </>
  )

  return (
    <nav
      ref={rootRef}
      className="fixed inset-x-0 top-0 z-50 w-full px-4 py-3 sm:px-6 sm:py-4"
    >
      <div className="mx-auto w-full max-w-[1400px]">
        {/* Desktop Navigation */}
        <div
          data-nav-desktop
          className="relative mx-auto hidden lg:block"
          onMouseLeave={(e) => {
            closeDesktopMenu()
            hidePill(e.currentTarget as HTMLElement)
          }}
          onKeyDown={(e) => {
            if (e.key === "Escape") closeDesktopMenu()
          }}
        >
          {/* Nav Container：页顶全宽展开，滚动后收缩为居中 pill；两态都磨砂+边框+阴影 */}
          <div
            className={cn(
              "overflow-hidden rounded-3xl border border-border/70 bg-card/55 shadow-lg backdrop-blur-2xl backdrop-saturate-150 transition-[background-color,border-color,box-shadow] duration-300 motion-reduce:transition-none",
              scrolled ? "mx-auto w-fit" : "mx-auto w-full max-w-6xl"
            )}
          >
            {/* Main Nav Bar */}
            <div className="flex items-center justify-between gap-2 py-2 pr-2.5 pl-5">
              {/* Brand */}
              <TransitionLink
                href={brand.href ?? "/"}
                className="mr-6 flex items-center gap-2 text-xl font-semibold tracking-tight text-foreground"
              >
                {brandContent}
              </TransitionLink>

              {/* Nav Links */}
              <div className="relative flex items-center gap-1">
                <span
                  data-nav-pill
                  aria-hidden
                  className="pointer-events-none absolute top-1/2 left-0 h-8 w-0 -translate-y-1/2 rounded-full bg-muted opacity-0"
                />
                {menus.map((menu, index) => {
                  const menuClass =
                    "relative flex items-center gap-1 rounded-full px-4 py-2 text-sm font-medium tracking-tight text-muted-foreground no-underline hover:text-foreground"
                  const chevron = (
                    <ChevronDown
                      aria-hidden
                      className={cn(
                        "size-3 transition-transform duration-200",
                        activeMenu === index && "rotate-180"
                      )}
                    />
                  )
                  const open = (e: React.MouseEvent<HTMLElement>) => {
                    const panel = desktopPanelRef.current
                    if (panel) {
                      // 顶掉进行中的关闭 tween 并清掉它的内联残留，
                      // 否则其 onComplete 会误关刚打开的菜单
                      gsap.killTweensOf(panel)
                      gsap.set(panel, { clearProps: "height,opacity" })
                    }
                    setActiveMenu(index)
                    movePill(e.currentTarget)
                  }
                  return menu.href ? (
                    <TransitionLink
                      key={menu.label}
                      href={menu.href}
                      onMouseEnter={open}
                      className={menuClass}
                    >
                      {menu.label}
                      {chevron}
                    </TransitionLink>
                  ) : (
                    <button
                      key={menu.label}
                      onMouseEnter={open}
                      onFocus={() => setActiveMenu(index)}
                      aria-haspopup="true"
                      aria-expanded={activeMenu === index}
                      className={menuClass}
                    >
                      {menu.label}
                      {chevron}
                    </button>
                  )
                })}
                {links.map((link) => (
                  <TransitionLink
                    key={link.label}
                    href={link.href}
                    className="relative rounded-full px-4 py-2 text-sm font-medium tracking-tight text-muted-foreground no-underline hover:text-foreground"
                    onMouseEnter={(e) => {
                      if (activeMenu !== null) closeDesktopMenu()
                      movePill(e.currentTarget as HTMLElement)
                    }}
                  >
                    {link.label}
                  </TransitionLink>
                ))}
              </div>

              {/* Right Side Actions */}
              {cta || actions ? (
                <div className="ml-6 flex items-center gap-2">
                  {actions}
                  {cta ? (
                    <TransitionLink
                      href={cta.href}
                      className="rounded-lg bg-primary px-5 py-2 text-sm font-medium tracking-tight text-primary-foreground no-underline hover:bg-primary/80"
                      onMouseEnter={() =>
                        activeMenu !== null && closeDesktopMenu()
                      }
                    >
                      {cta.label}
                    </TransitionLink>
                  ) : null}
                </div>
              ) : null}
            </div>

            {/* Expanded Content */}
            {activeMenu !== null && menus[activeMenu] ? (
              <div
                ref={desktopPanelRef}
                className="overflow-hidden"
                style={{ height: 0 }}
              >
                <div className="p-2">
                  <div className="grid w-[620px] grid-cols-2 gap-3">
                    {menus[activeMenu].items.map((item) => (
                      <NavMenuItemCard key={item.title} item={item} />
                    ))}
                  </div>
                </div>
              </div>
            ) : null}
          </div>
        </div>

        {/* Mobile Navigation */}
        <div data-nav-mobile className="lg:hidden">
          <div className="overflow-hidden rounded-3xl border border-border/70 bg-card/55 shadow-lg backdrop-blur-2xl backdrop-saturate-150">
            {/* Mobile Nav Bar */}
            <div className="flex items-center justify-between py-2.5 pr-2.5 pl-4">
              {/* Brand */}
              <TransitionLink
                href={brand.href ?? "/"}
                className="flex items-center gap-2 text-xl font-semibold tracking-tight text-foreground"
              >
                {brandContent}
              </TransitionLink>

              {/* Mobile Menu Button */}
              <button
                onClick={toggleMobileMenu}
                className="flex h-10 w-10 items-center justify-center rounded-lg bg-primary text-primary-foreground"
                aria-label={mobileMenuOpen ? "Close menu" : "Open menu"}
                aria-expanded={mobileMenuOpen}
              >
                {mobileMenuOpen ? (
                  <X className="h-5 w-5" />
                ) : (
                  <Menu className="h-5 w-5" />
                )}
              </button>
            </div>

            {/* Mobile Expanded Content */}
            {mobileMenuOpen ? (
              <div
                ref={mobilePanelRef}
                className="overflow-hidden"
                style={{ height: 0 }}
              >
                <div className="px-4 pt-2 pb-4">
                  {/* Mobile Menu Content */}
                  <div className="space-y-4">
                    {/* Simple Links */}
                    {links.length > 0 && (
                      <div className="space-y-1">
                        {links.map((link) => (
                          <TransitionLink
                            key={link.label}
                            href={link.href}
                            className="block px-2 py-2 text-sm font-medium text-foreground no-underline"
                          >
                            {link.label}
                          </TransitionLink>
                        ))}
                      </div>
                    )}

                    {/* Mobile Actions（主题切换、设置等图标操作） */}
                    {actions ? (
                      <div className="flex items-center gap-2 px-2">
                        {actions}
                      </div>
                    ) : null}

                    {/* Mobile CTA */}
                    {cta ? (
                      <div>
                        <TransitionLink
                          href={cta.href}
                          className="block w-full rounded-full bg-primary px-6 py-2.5 text-center text-sm font-medium text-primary-foreground no-underline"
                        >
                          {cta.label}
                        </TransitionLink>
                      </div>
                    ) : null}

                    {/* Menu Sections */}
                    {menus.map((menu) => (
                      <div key={menu.label} className="border-border pt-2">
                        <h3 className="mb-2 px-2 text-sm font-bold text-foreground">
                          {menu.href ? (
                            <TransitionLink
                              href={menu.href}
                              className="no-underline"
                            >
                              {menu.label}
                            </TransitionLink>
                          ) : (
                            menu.label
                          )}
                        </h3>
                        <div className="space-y-2">
                          {menu.items.map((item) => (
                            <NavMenuItemCard
                              key={item.title}
                              item={item}
                              size="sm"
                            />
                          ))}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            ) : null}
          </div>
        </div>
      </div>
    </nav>
  )
}

export default SiteNav
