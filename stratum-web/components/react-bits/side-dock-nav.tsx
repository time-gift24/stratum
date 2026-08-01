"use client"

import { useRef } from "react"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"

import { scrollToHash } from "@/lib/scroll-to"
import { TransitionLink } from "@/components/chrome/page-transition"

/**
 * SideDockNav —— 左侧竖排图标 dock（reactbits navigation-4 改造，桌面端）。
 * 导航项数据驱动，由调用方传入；个数不限，全部动态计算。
 * `#` 锚点点击经 GSAP ScrollToPlugin 平滑滚动（见 lib/scroll-to.ts）。
 * 颜色只消费最外层 token：每个按钮独立成块——半透明 tile（card/50 + border +
 * backdrop-blur），hover = bg-primary/15 + text-primary，tooltip 用 popover。
 *
 * 动效全部 GSAP，正逻辑驱动：
 * - 悬浮上某个 item 才激活波形，且以该 item 的中心为锚点——项内移动不再改变波形；
 * - 波形 = 高斯梯度（σ 由列表总长推导）+ gsap.ticker 逐帧弹簧积分，
 *   写真实 width/height，dock 高度随波动动态伸缩，邻居被布局自然推开；
 * - 锚定项中心钉在其激活时的视觉位置（dock-lock），动态高度不会让 hover 漂移丢失；
 * - 图标悬浮突出：back.out 回弹放大 + tile 上浮（颜色/光晕走 CSS hover + token）。
 * tooltip 为纯 CSS group-hover（常驻 DOM，无 React 重渲染）。
 *
 * 注意：items 需要稳定引用（模块级常量或 useMemo）——弹簧状态与 ticker
 * 注册以 items 为依赖，内联数组会让每次渲染都重建并硬重置进行中的波形。
 */

export interface SideDockNavItem {
  icon: React.ComponentType<{ className?: string }>
  label: string
  href: string
}

const BASE_SIZE = 56 // 静止尺寸（px）
const GAP = 12 // 与 nav 的 gap-3 保持一致
const MAX_SCALE = 1.2 // 凸起峰值放大倍率（被悬浮项）
// 弹簧参数：stiffness 大=跟手；damping 接近临界值 2√stiffness≈23.7，
// 太小会欠阻尼过冲——目标阶跃时其他项会先抖一下
const STIFFNESS = 140
const DAMPING = 20

export function SideDockNav({ items }: { items: SideDockNavItem[] }) {  const rootRef = useRef<HTMLDivElement>(null)
  const navRef = useRef<HTMLElement>(null)
  const itemRefs = useRef<(HTMLDivElement | null)[]>([])
  const springs = useRef<{ x: number; v: number }[]>([])
  const targets = useRef<number[]>([])
  const awake = useRef(false)
  // 锚定状态：被悬浮项的索引 + 激活时它的视觉中心（含当时的波形与平移）
  const pinned = useRef<{ index: number; center: number } | null>(null)
  const shift = useRef(0)

  const { contextSafe } = useGSAP(
    () => {
      springs.current = items.map(() => ({ x: 1, v: 0 }))
      targets.current = items.map(() => 1)

      gsap.from(rootRef.current, {
        opacity: 0,
        x: -20,
        duration: 0.5,
        ease: "power2.out",
      })
      gsap.from(itemRefs.current, {
        opacity: 0,
        y: 20,
        duration: 0.3,
        delay: 0.3,
        stagger: 0.1,
        ease: "power2.out",
      })

      const tick = (_time: number, deltaTime: number) => {
        if (!awake.current) return
        const dt = Math.min(deltaTime / 1000, 0.064)
        let settled = true

        // 单趟积分并缓存尺寸，pinned 分支直接复用（js-combine-iterations）
        const sizes = new Array<number>(items.length)
        itemRefs.current.forEach((el, i) => {
          const spring = springs.current[i]
          const target = targets.current[i]
          spring.v += ((target - spring.x) * STIFFNESS - spring.v * DAMPING) * dt
          spring.x += spring.v * dt
          if (Math.abs(spring.v) > 0.0005 || Math.abs(target - spring.x) > 0.0005) {
            settled = false
          }
          const size = BASE_SIZE * spring.x
          sizes[i] = size
          if (el) gsap.set(el, { width: size, height: size })
        })

        const nav = navRef.current
        if (nav) {
          if (pinned.current) {
            // 锚定项中心钉在激活时的视觉位置（复用上趟缓存的尺寸）
            const k = pinned.current.index
            let totalH = GAP * (items.length - 1)
            let centerK = 0
            for (let j = 0; j < items.length; j++) {
              const size = sizes[j]
              totalH += size
              if (j < k) centerK += size + GAP
              else if (j === k) centerK += size / 2
            }
            shift.current =
              pinned.current.center -
              (window.innerHeight / 2 - totalH / 2 + centerK)
          } else {
            // 光标离开：随收缩动画指数释放回中
            shift.current += (0 - shift.current) * Math.min(1, dt * 10)
            if (Math.abs(shift.current) > 0.5) settled = false
          }
          gsap.set(nav, { y: shift.current })
        }

        if (settled) awake.current = false
      }
      gsap.ticker.add(tick)
      return () => gsap.ticker.remove(tick)
    },
    { scope: rootRef, dependencies: [items] }
  )

  // 正逻辑：悬浮上第 index 项 → 波定在该项中心；项内移动不再改变波形
  const activate = (index: number) => {
    const el = itemRefs.current[index]
    if (!el) return
    awake.current = true

    const box = el.getBoundingClientRect()
    pinned.current = { index, center: box.top + box.height / 2 }

    // σ 由列表总长动态推导：任何 item 个数都形成"中间大、两头小"的整列梯度
    const slot = BASE_SIZE + GAP
    const sigma = (items.length * slot) / 4
    targets.current = items.map((_, i) => {
      const distance = ((i - index) * slot) / sigma
      const gauss = Math.exp(-(distance * distance))
      return 1 + (MAX_SCALE - 1) * gauss
    })
  }

  const deactivate = () => {
    awake.current = true
    pinned.current = null
    targets.current.fill(1)
  }

  // 悬浮突出：图标回弹放大 + tile 轻微上浮；颜色与光晕由 CSS hover 负责（token 驱动）
  const hoverPop = contextSafe((tile: HTMLElement, entering: boolean) => {
    gsap.to(tile.querySelector("[data-dock-icon]"), {
      scale: entering ? 1.25 : 1,
      duration: entering ? 0.35 : 0.25,
      ease: entering ? "back.out(3)" : "power2.out",
    })
    gsap.to(tile, {
      y: entering ? -3 : 0,
      duration: 0.25,
      ease: "power2.out",
    })
  })

  return (
    <div
      ref={rootRef}
      className="fixed top-1/2 left-4 z-50 hidden -translate-y-1/2 md:block"
    >
      <nav
        ref={navRef}
        aria-label="页面导航"
        className="flex flex-col items-center gap-3"
        onMouseLeave={deactivate}
      >
        {items.map((item, index) => (
          <NavItem
            key={item.label}
            ref={(el) => {
              itemRefs.current[index] = el
            }}
            item={item}
            onActivate={() => activate(index)}
            onHoverPop={hoverPop}
          />
        ))}
      </nav>
    </div>
  )
}

function NavItem({
  ref,
  item,
  onActivate,
  onHoverPop,
}: {
  ref: React.Ref<HTMLDivElement>
  item: SideDockNavItem
  onActivate: () => void
  onHoverPop: (tile: HTMLElement, entering: boolean) => void
}) {
  const Icon = item.icon

  return (
    <div
      ref={ref}
      className="group relative flex items-center justify-center"
      style={{ width: BASE_SIZE, height: BASE_SIZE }}
      onMouseEnter={(e) => {
        onActivate()
        onHoverPop(e.currentTarget.querySelector("a") as HTMLElement, true)
      }}
      onMouseLeave={(e) => {
        onHoverPop(e.currentTarget.querySelector("a") as HTMLElement, false)
      }}
    >
      <TransitionLink
        href={item.href}
        aria-label={item.label}
        onClick={(e) => {
          if (!item.href.startsWith("#")) return
          e.preventDefault()
          scrollToHash(item.href)
          window.history.replaceState(null, "", item.href)
        }}
        className="flex h-full w-full items-center justify-center rounded-2xl border border-border/60 bg-card/50 no-underline shadow-[0_4px_16px] shadow-black/5 backdrop-blur-md transition-colors hover:border-primary/40 hover:bg-primary/15"
      >
        <span
          data-dock-icon
          className="text-muted-foreground transition-[color,filter] duration-200 group-hover:text-primary group-hover:[filter:drop-shadow(0_0_8px_var(--primary))]"
        >
          <Icon className="h-5 w-5" />
        </span>
      </TransitionLink>

      {/* Tooltip：常驻 DOM，纯 CSS group-hover 显隐，不产生 React 重渲染 */}
      <div
        role="tooltip"
        className="pointer-events-none absolute top-1/2 left-full ml-4 -translate-x-2 -translate-y-1/2 rounded-lg border border-border bg-popover px-3 py-2 text-sm font-medium whitespace-nowrap text-popover-foreground opacity-0 shadow-lg transition-[opacity,translate] duration-200 group-hover:translate-x-0 group-hover:opacity-100"
      >
        {item.label}
      </div>
    </div>
  )
}

export default SideDockNav
