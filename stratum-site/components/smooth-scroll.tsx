"use client";

/**
 * Lenis 平滑滚动：流体量级的滚动手感。
 * reduced-motion 时完全不启用，回退原生滚动。
 */
import Lenis from "lenis";
import { useEffect, type ReactNode } from "react";

const LENIS_OPTIONS = {
  duration: 1.4,
  easing: (t: number) => Math.min(1, 1.001 - Math.pow(2, -10 * t)),
  orientation: "vertical" as const,
  smoothWheel: true,
  wheelMultiplier: 1,
  touchMultiplier: 1.6,
};

export function SmoothScroll({ children }: { children: ReactNode }): ReactNode {
  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;

    const lenis = new Lenis(LENIS_OPTIONS);
    let frame = 0;

    function raf(time: number) {
      lenis.raf(time);
      frame = requestAnimationFrame(raf);
    }

    frame = requestAnimationFrame(raf);

    function handleAnchorClick(e: MouseEvent) {
      const anchor = (e.target as HTMLElement).closest('a[href^="#"]');
      if (!anchor) return;
      const href = anchor.getAttribute("href");
      if (!href || href === "#") return;
      const element = document.querySelector(href);
      if (!element) return;
      e.preventDefault();
      const target = element as HTMLElement;
      lenis.scrollTo(target, { offset: -24 });
      // 焦点随锚点跳转（键盘/读屏上下文同步）；main/section 容器无 tabindex 时先补 -1
      if (
        !target.hasAttribute("tabindex") &&
        (target.tagName === "MAIN" || target.tagName === "SECTION")
      ) {
        target.setAttribute("tabindex", "-1");
      }
      target.focus({ preventScroll: true });
    }

    document.addEventListener("click", handleAnchorClick);

    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("click", handleAnchorClick);
      lenis.destroy();
    };
  }, []);

  return <>{children}</>;
}
