"use client";

/**
 * App 顶栏：与首页导航同一副磨砂玻璃 pill 材质（纸白 72% + blur + 软阴影），
 * 进入时从窄 pill 丝滑展开宽度（38% → 100%），ease-fluid 量级。
 */
import { LanguageToggle } from "@/components/ui/language-toggle";
import { siteConfig } from "@/lib/config";
import { useLanguage } from "@/lib/i18n";
import { easeFluid } from "@/lib/motion";
import { Menu } from "lucide-react";
import { motion } from "motion/react";
import Link from "next/link";
import type { ReactNode } from "react";

type AppTopBarProps = {
  onMenuClick: () => void;
};

export function AppTopBar({ onMenuClick }: AppTopBarProps): ReactNode {
  const { t } = useLanguage();

  return (
    <header className="pointer-events-none fixed inset-x-0 top-0 z-40 pt-3 pr-safe-r pl-safe-l sm:px-6">
      <motion.div
        initial={{ width: "38%" }}
        animate={{ width: "100%" }}
        transition={{ duration: 0.9, ease: [...easeFluid], delay: 0.1 }}
        className="rounded-pill pointer-events-auto mx-auto border border-ink/8 bg-paper/72 py-2.5 shadow-card backdrop-blur-md"
      >
        <nav className="flex items-center justify-between px-5">
          <div className="flex items-center gap-3">
            <button
              type="button"
              onClick={onMenuClick}
              aria-label={t.conversation.menu}
              className="-my-2 flex cursor-pointer items-center py-2 text-ink-soft transition-colors hover:text-ink lg:hidden"
            >
              <Menu size={20} aria-hidden />
            </button>
            <Link
              href="/"
              translate="no"
              className="-my-3 py-3 font-display text-ui font-bold whitespace-nowrap text-ink"
            >
              运筹 STRATUM
            </Link>
            <span aria-hidden className="hidden text-ink-soft/50 sm:inline">
              /
            </span>
            <span className="tracking-label hidden font-mono text-xs whitespace-nowrap text-ink-soft uppercase sm:inline">
              {t.conversation.title}
            </span>
          </div>
          <div className="flex items-center gap-5">
            <a
              href={siteConfig.githubUrl}
              target="_blank"
              rel="noreferrer"
              className="tracking-label -my-3 py-3 font-mono text-xs text-ink-soft uppercase transition-colors hover:text-ink"
            >
              {t.nav.github}
            </a>
            <LanguageToggle />
          </div>
        </nav>
      </motion.div>
    </header>
  );
}
