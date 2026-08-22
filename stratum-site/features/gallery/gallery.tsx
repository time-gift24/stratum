"use client";

/**
 * 组件展示页：以纸墨主题实拍底座的色板、字体与全部公共组件。
 * 参照 ui.shadcn.com/create 的陈列方式——每个组件一块实拍展区，mono 标签命名。
 */
import MoltenMetal from "@/components/react-bits/molten-metal";
import { Button, ButtonLink } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Chip } from "@/components/ui/chip";
import { Dropdown } from "@/components/ui/dropdown";
import { LanguageToggle } from "@/components/ui/language-toggle";
import { NavTree } from "@/components/ui/nav-tree";
import { PromptBox } from "@/components/ui/prompt-box";
import { SectionTitle } from "@/components/ui/section";
import { TerminalCard } from "@/components/ui/terminal-card";
import { TraceRibbon } from "@/components/ui/trace-ribbon";
import { useLanguage } from "@/lib/i18n";
import { Blocks, MessageSquarePlus, Sparkles } from "lucide-react";
import Link from "next/link";
import type { ReactNode } from "react";

const COLOR_TOKENS = [
  "paper",
  "paper-dim",
  "mist",
  "ink",
  "ink-soft",
  "abyss",
  "abyss-raise",
  "bone",
  "fog",
  "seal",
  "seal-deep",
  "bronze",
] as const;

/** 展区框：mono 标签 + 实拍区。dark 用于深处语境的组件。 */
function Exhibit({
  label,
  dark = false,
  children,
}: {
  label: string;
  dark?: boolean;
  children: ReactNode;
}): ReactNode {
  return (
    <section>
      <h2 className="tracking-label mb-4 font-mono text-xs font-medium text-ink-soft uppercase">
        {label}
      </h2>
      <div
        className={
          "rounded-card p-6 sm:p-10 " +
          (dark ? "on-abyss bg-abyss" : "bg-paper-dim")
        }
      >
        {children}
      </div>
    </section>
  );
}

export function Gallery(): ReactNode {
  const { t } = useLanguage();
  const s = t.gallery.sections;

  return (
    <div className="min-h-svh bg-paper">
      <header className="mx-auto flex w-full max-w-shell items-center justify-between px-6 py-8 sm:px-10">
        <Link
          href="/"
          className="-my-3 py-3 font-mono text-xs tracking-label text-ink-soft uppercase transition-colors hover:text-ink"
        >
          ← {t.gallery.back}
        </Link>
        <LanguageToggle />
      </header>

      <main
        id="main"
        tabIndex={-1}
        className="mx-auto w-full max-w-shell space-y-20 px-6 pt-10 pb-24 sm:px-10"
      >
        <div>
          <p className="tracking-eyebrow mb-6 font-mono text-xs font-medium text-ink uppercase">
            {t.gallery.eyebrow}
          </p>
          <h1 className="font-display text-display-sm font-black tracking-display text-balance text-ink">
            {t.gallery.title}
          </h1>
          <p className="leading-prose mt-5 max-w-[52ch] text-lead text-ink-soft">
            {t.gallery.sub}
          </p>
        </div>

        {/* 色彩 */}
        <Exhibit label={s.colors}>
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
            {COLOR_TOKENS.map((token) => (
              <div key={token}>
                <div
                  className="h-16 rounded-xl"
                  style={{ backgroundColor: `var(--color-${token})` }}
                />
                <p className="mt-2 font-mono text-xs text-ink-soft">{token}</p>
              </div>
            ))}
          </div>
        </Exhibit>

        {/* 排版 */}
        <Exhibit label={s.type}>
          <div className="space-y-6">
            <p className="font-display text-display font-black tracking-display text-ink">
              把事做完 <em className="font-serif font-normal italic">done</em>
            </p>
            <p className="font-display text-headline font-bold tracking-display text-ink">
              段落标题 Headline
            </p>
            <p className="leading-prose max-w-[52ch] text-lead text-ink-soft">
              正文 Lead — 对话发起，Runtime 推进。审批、取消、恢复都在你手里。
            </p>
            <p className="text-ui text-ink">控件文字 UI 15px</p>
            <p className="tracking-eyebrow font-mono text-xs font-medium text-ink uppercase">
              Eyebrow · Chivo Mono 12px
            </p>
          </div>
        </Exhibit>

        {/* 按钮 */}
        <Exhibit label={s.buttons}>
          <div className="flex flex-wrap items-center gap-4">
            <Button>Primary</Button>
            <Button variant="ghost">Ghost</Button>
            <ButtonLink href="/components">Link 形态</ButtonLink>
          </div>
        </Exhibit>

        {/* 按钮 · 深处 */}
        <Exhibit label={`${s.buttons} · Abyss`} dark>
          <Button variant="abyss">Abyss Primary</Button>
        </Exhibit>

        {/* 卡片 */}
        <Exhibit label={s.cards}>
          <div className="grid gap-5 sm:grid-cols-2">
            <Card>
              <h3 className="text-lg font-bold">纸面卡片</h3>
              <p className="leading-prose mt-3 text-ui text-ink-soft">
                paper-dim 地，hover 上浮并加深阴影。
              </p>
            </Card>
            <div className="rounded-card bg-abyss p-3">
              <Card tone="abyss" className="h-full">
                <h3 className="text-lg font-bold text-bone">深处卡片</h3>
                <p className="leading-prose mt-3 text-ui text-fog">
                  abyss-raise 地，服务下潜段。
                </p>
              </Card>
            </div>
          </div>
        </Exhibit>

        {/* 状态标签 */}
        <Exhibit label={s.chips} dark>
          <div className="flex flex-wrap gap-3">
            <Chip state="running">RUNNING</Chip>
            <Chip state="approval">AWAITING APPROVAL</Chip>
            <Chip>LEDGER SYNCED</Chip>
          </div>
        </Exhibit>

        {/* 对话盒 */}
        <Exhibit label={s.prompt}>
          <PromptBox />
        </Exhibit>

        {/* 终端卡 */}
        <Exhibit label={s.terminal} dark>
          <TerminalCard
            title={t.depth.quickstartTitle}
            note={t.depth.quickstartNote}
            lines={[
              "git clone <repo> && cd stratum",
              "docker compose up -d",
              "cargo run --release -p stratum-api",
            ]}
          />
        </Exhibit>

        {/* 执行轨迹 */}
        <Exhibit label={s.trace} dark>
          <TraceRibbon />
        </Exhibit>

        {/* 段落标题 */}
        <Exhibit label="SectionTitle">
          <SectionTitle
            titleA={t.mechanism.titleA}
            accent={t.mechanism.titleAccent}
          />
        </Exhibit>

        {/* 导航树 */}
        <Exhibit label="NavTree">
          <div className="flex gap-10">
            <NavTree
              sections={[
                {
                  id: "demo",
                  label: "Text Animations",
                  items: [
                    { id: "a", label: "Masked Heading", badge: "NEW" },
                    { id: "b", label: "Particle Text", active: true },
                    { id: "c", label: "Split Flap Text" },
                  ],
                },
                {
                  id: "demo2",
                  label: "Tools",
                  items: [
                    { id: "d", label: "Background Studio" },
                    { id: "e", label: "Shape Magic" },
                  ],
                },
              ]}
            />
            <NavTree
              collapsed
              sections={[
                {
                  id: "demo",
                  items: [
                    { id: "a", label: "Masked Heading", icon: <Blocks size={15} /> },
                    {
                      id: "b",
                      label: "Particle Text",
                      icon: <MessageSquarePlus size={15} />,
                      active: true,
                    },
                  ],
                },
              ]}
            />
          </div>
        </Exhibit>

        {/* Dropdown */}
        <Exhibit label="Dropdown">
          <div className="flex flex-wrap items-center gap-3">
            <Dropdown
              label="模型"
              options={t.conversation.models}
              value={t.conversation.models[0].name}
              onChange={() => {}}
            />
            <Dropdown
              label={t.conversation.suggestionsLabel}
              options={t.hero.suggestions.map((s) => ({ name: s }))}
              value=""
              onChange={() => {}}
              icon={<Sparkles size={14} aria-hidden />}
            />
          </div>
        </Exhibit>

        {/* 熔墨背景 */}
        <Exhibit label={s.ink}>
          <div className="relative h-72 overflow-hidden rounded-card bg-paper">
            <MoltenMetal
              color1="#e2d9c5"
              color2="#4a4238"
              color3="#b24731"
              speed={0.25}
              scale={2.6}
              glow={2.2}
              coreSize={0.07}
              blackPoint={0.03}
              brightness={1.35}
              opacity={0.9}
              grain
              grainIntensity={0.04}
            />
          </div>
        </Exhibit>
      </main>
    </div>
  );
}
