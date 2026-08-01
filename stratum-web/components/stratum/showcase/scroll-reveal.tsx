"use client"

import * as React from "react"

import { useGSAP } from "@gsap/react"
import gsap from "gsap"
import { ScrollTrigger } from "gsap/ScrollTrigger"

gsap.registerPlugin(ScrollTrigger, useGSAP)

/**
 * ScrollReveal —— 滚动进入视口时的 reveal 动画容器。
 * 上移 + 淡入，回滚离开时反向播放；尊重 prefers-reduced-motion。
 */
function ScrollReveal({
  className,
  children,
  ...props
}: React.ComponentProps<"div">) {
  const ref = React.useRef<HTMLDivElement>(null)

  useGSAP(
    () => {
      const el = ref.current
      if (!el) return
      const mm = gsap.matchMedia()
      mm.add("(prefers-reduced-motion: no-preference)", () => {
        gsap.from(el, {
          y: 36,
          autoAlpha: 0,
          duration: 0.9,
          ease: "power3.out",
          scrollTrigger: {
            trigger: el,
            start: "top 88%",
            toggleActions: "play none none reverse",
          },
        })
      })
    },
    { scope: ref }
  )

  return (
    <div ref={ref} data-slot="scroll-reveal" className={className} {...props}>
      {children}
    </div>
  )
}

export { ScrollReveal }
