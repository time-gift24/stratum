import gsap from "gsap"
import { ScrollToPlugin } from "gsap/ScrollToPlugin"

gsap.registerPlugin(ScrollToPlugin)

/** 与 ShowcaseSection 的 scroll-mt-20 对齐的顶部偏移。 */
const OFFSET_Y = 80

/**
 * scrollToHash —— 平滑滚动到 `#id` 锚点。
 * 时长随滚动距离自适应（clamp 在 0.5–1.2s），power3.inOut 缓动；
 * prefers-reduced-motion 时直接瞬时定位。
 */
export function scrollToHash(hash: string) {
  const el = document.querySelector(hash)
  if (!el) return

  const targetY = el.getBoundingClientRect().top + window.scrollY - OFFSET_Y

  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    window.scrollTo(0, targetY)
    return
  }

  const distance = Math.abs(targetY - window.scrollY)
  gsap.to(window, {
    scrollTo: { y: targetY },
    duration: gsap.utils.clamp(0.5, 1.2, distance / 1500),
    ease: "power3.inOut",
    overwrite: "auto",
  })
}
