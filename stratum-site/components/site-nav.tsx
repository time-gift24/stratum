"use client";

import { LanguageToggle } from "@/components/ui/language-toggle";
import { cn } from "@/lib/cn";
import { siteConfig } from "@/lib/config";
import { useLanguage } from "@/lib/i18n";
import { useEffect, useRef, useState, type ReactNode } from "react";

/**
 * 顶栏。静止在页顶时是纯文字；向下滚动渐变为磨砂悬浮玻璃条：
 * 透明度 / backdrop-blur / 阴影随滚动进度（0-96px 区间）连续插值，
 * 玻璃配色跟随下潜明暗两态。reduced-motion 下同样生效（这是材质变化，不是动画编排）。
 */
export function SiteNav(): ReactNode {
  const { t } = useLanguage();
  const [deep, setDeep] = useState(false);
  const [floating, setFloating] = useState(false);
  const barRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let ticking = false;
    // 派生布尔只在跨越阈值时 setState，其余滚动帧只走 ref 直改 DOM
    let lastDeep = false;
    let lastFloating = false;

    function measure() {
      ticking = false;
      const scrollY = window.scrollY;
      // 磨砂进度：前 96px 滚动内从 0 渐变到 1
      const p = Math.min(1, scrollY / 96);

      const docH = document.documentElement.scrollHeight;
      const mid = (scrollY + window.innerHeight * 0.5) / docH;
      const isDeep = mid > 0.54 && mid < 0.7;
      const isFloating = p > 0.02;
      if (isDeep !== lastDeep) {
        lastDeep = isDeep;
        setDeep(isDeep);
      }
      if (isFloating !== lastFloating) {
        lastFloating = isFloating;
        setFloating(isFloating);
      }

      const bar = barRef.current;
      if (!bar) return;
      const glass = isDeep
        ? `rgb(22 18 14 / ${(0.62 * p).toFixed(3)})`
        : `rgb(251 244 231 / ${(0.72 * p).toFixed(3)})`;
      bar.style.backgroundColor = glass;
      bar.style.backdropFilter = `blur(${(14 * p).toFixed(1)}px)`;
      bar.style.setProperty(
        "-webkit-backdrop-filter",
        `blur(${(14 * p).toFixed(1)}px)`,
      );
      bar.style.boxShadow =
        p > 0.02
          ? `0 8px 32px rgb(18 16 14 / ${(0.12 * p).toFixed(3)})`
          : "none";
      bar.style.borderColor = isDeep
        ? `rgb(239 236 228 / ${(0.12 * p).toFixed(3)})`
        : `rgb(28 25 21 / ${(0.08 * p).toFixed(3)})`;
    }

    function onScroll() {
      if (!ticking) {
        ticking = true;
        requestAnimationFrame(measure);
      }
    }

    measure();
    window.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onScroll);
    return () => {
      window.removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onScroll);
    };
  }, []);

  const linkClass =
    "inline-flex items-center py-3.5 -my-3.5 opacity-70 transition-opacity hover:opacity-100";

  return (
    <header className="pointer-events-none fixed inset-x-0 top-0 z-40 pt-3 pr-safe-r pl-safe-l sm:px-6">
      <div
        ref={barRef}
        className={cn(
          "rounded-pill pointer-events-auto mx-auto max-w-shell border border-transparent transition-[padding] duration-300",
          floating ? "py-2.5" : "py-3.5",
        )}
      >
        <nav
          className={cn(
            "flex w-full items-center justify-between px-4 transition-colors duration-300 sm:px-5",
            deep ? "text-bone" : "text-ink",
          )}
        >
          <a
            href="#main"
            translate="no"
            className="-my-3.5 py-3.5 font-display text-ui font-bold"
          >
            运筹 STRATUM
          </a>
          <div className="tracking-label flex items-center gap-6 font-mono text-xs uppercase">
            <a
              href="#mechanism"
              className={cn(linkClass, "hidden sm:inline-flex")}
            >
              {t.nav.mechanism}
            </a>
            <a href="#depth" className={cn(linkClass, "hidden sm:inline-flex")}>
              {t.nav.depth}
            </a>
            <a
              href="#quickstart"
              className={cn(linkClass, "hidden sm:inline-flex")}
            >
              {t.nav.quickstart}
            </a>
            <a
              href={siteConfig.githubUrl}
              target="_blank"
              rel="noreferrer"
              className={linkClass}
            >
              {t.nav.github}
            </a>
            <LanguageToggle />
          </div>
        </nav>
      </div>
    </header>
  );
}
