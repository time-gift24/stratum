"use client";

import { ButtonLink } from "@/components/ui/button";
import { Section } from "@/components/ui/section";
import { siteConfig } from "@/lib/config";
import { useLanguage } from "@/lib/i18n";
import { Reveal } from "@/lib/motion";
import type { ReactNode } from "react";

/** 浮回纸面：最终行动。 */
export function FinalCta(): ReactNode {
  const { t } = useLanguage();

  return (
    <Section id="cta" className="text-center">
      <Reveal>
        <p className="tracking-eyebrow font-mono text-xs font-medium text-ink-soft uppercase">
          {t.cta.eyebrow}
        </p>
        <h2 className="text-display-sm mx-auto mt-6 max-w-[18ch] font-display font-black tracking-display text-balance text-ink">
          {t.cta.titleA}
          <em className="font-serif font-normal italic">{t.cta.titleAccent}</em>
        </h2>
        <p className="leading-prose mx-auto mt-5 max-w-[44ch] text-lead text-ink-soft">
          {t.cta.sub}
        </p>
        <div className="mt-10 flex flex-wrap items-center justify-center gap-4">
          <ButtonLink
            href={siteConfig.githubUrl}
            target="_blank"
            rel="noreferrer"
          >
            {t.cta.primary} →
          </ButtonLink>
          <ButtonLink variant="ghost" href="#quickstart">
            {t.cta.secondary}
          </ButtonLink>
        </div>
      </Reveal>
    </Section>
  );
}
