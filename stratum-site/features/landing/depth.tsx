"use client";

import { Card } from "@/components/ui/card";
import { Chip } from "@/components/ui/chip";
import { Section, SectionTitle } from "@/components/ui/section";
import { TerminalCard } from "@/components/ui/terminal-card";
import { TraceRibbon } from "@/components/ui/trace-ribbon";
import { useLanguage } from "@/lib/i18n";
import { Reveal } from "@/lib/motion";
import type { ReactNode } from "react";

const QUICKSTART_LINES = [
  "git clone <repo> && cd stratum",
  "docker compose up -d",
  "cargo run --release -p stratum-api",
];

/** 深处：架构事实 + 执行轨迹 + 自托管命令。 */
export function Depth(): ReactNode {
  const { t } = useLanguage();

  return (
    <Section id="depth" eyebrow={t.depth.eyebrow} onAbyss>
      <SectionTitle
        titleA={t.depth.titleA}
        accent={t.depth.titleAccent}
        onAbyss
      />

      <div className="mt-14 grid gap-5 sm:grid-cols-2">
        {t.depth.facts.map((fact) => (
          <Reveal key={fact.title}>
            <Card tone="abyss" className="h-full">
              <h3 className="text-lg font-bold text-bone">{fact.title}</h3>
              <p className="leading-prose mt-3 text-ui text-fog">
                {fact.body}
              </p>
            </Card>
          </Reveal>
        ))}
      </div>

      <Reveal className="mt-20">
        <TraceRibbon />
        <div className="mt-5 flex flex-wrap items-center gap-3">
          <Chip state="running">{t.depth.traceRunning}</Chip>
          <Chip state="approval">{t.depth.traceApproval}</Chip>
          <Chip>{t.depth.traceSynced}</Chip>
        </div>
        <p className="tracking-label mt-4 font-mono text-xs text-fog">
          {t.depth.traceCaption}
        </p>
      </Reveal>

      <div id="quickstart" className="mt-20 scroll-mt-24">
        <Reveal>
          <TerminalCard
            title={t.depth.quickstartTitle}
            note={t.depth.quickstartNote}
            lines={QUICKSTART_LINES}
          />
        </Reveal>
      </div>
    </Section>
  );
}
