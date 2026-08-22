"use client";

import { siteConfig } from "@/lib/config";
import { useLanguage } from "@/lib/i18n";
import type { ReactNode } from "react";

export function SiteFooter(): ReactNode {
  const { t } = useLanguage();

  return (
    <footer className="mx-auto w-full max-w-shell px-6 pb-12 sm:px-10">
      <div className="flex flex-col items-start justify-between gap-6 border-t border-ink/10 pt-8 sm:flex-row sm:items-center">
        <div>
          <p className="font-display text-sm font-bold">{t.footer.rights}</p>
          <p className="mt-1 text-sm text-ink-soft">{t.footer.tagline}</p>
        </div>
        <div className="tracking-label flex items-center gap-2 font-mono text-xs text-ink-soft uppercase">
          <span
            aria-hidden
            className="inline-block h-2 w-2 rounded-xs bg-seal"
          />
          <a
            href={siteConfig.githubUrl}
            target="_blank"
            rel="noreferrer"
            className="-my-3 py-3 transition-colors hover:text-ink"
          >
            GitHub
          </a>
          <a
            href="/components"
            className="-my-3 py-3 transition-colors hover:text-ink"
          >
            {t.footer.gallery}
          </a>
        </div>
      </div>
    </footer>
  );
}
