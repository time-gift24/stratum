"use client";

import { PromptBox } from "@/components/ui/prompt-box";
import { useLanguage } from "@/lib/i18n";
import { fadeIn, fadeUp, stagger, useReducedMotion } from "@/lib/motion";
import { motion } from "motion/react";
import type { ReactNode } from "react";

/** 首屏：纸面大气场（熔墨背景在页面层）+ 悬浮对话入口。 */
export function Hero(): ReactNode {
  const { t } = useLanguage();
  const prefersReducedMotion = useReducedMotion();

  return (
    <section className="relative flex min-h-svh flex-col items-center justify-center overflow-hidden px-6">
      <motion.div
        initial="hidden"
        animate="visible"
        variants={prefersReducedMotion ? fadeIn : stagger}
        className="relative z-10 flex w-full flex-col items-center text-center"
      >
        <motion.p
          variants={prefersReducedMotion ? fadeIn : fadeUp}
          className="tracking-eyebrow mb-7 font-mono text-xs font-medium text-ink-soft uppercase"
        >
          {t.hero.eyebrow}
        </motion.p>
        <motion.h1
          variants={prefersReducedMotion ? fadeIn : fadeUp}
          className="max-w-[16ch] font-display text-display font-black tracking-display text-balance text-ink"
        >
          {t.hero.titleA}
          <em className="font-serif font-normal italic">{t.hero.titleAccent}</em>
          {t.hero.titleB}
        </motion.h1>
        <motion.p
          variants={prefersReducedMotion ? fadeIn : fadeUp}
          className="leading-prose mt-7 max-w-[46ch] text-lead text-ink-soft"
        >
          {t.hero.sub}
        </motion.p>
        <motion.div
          variants={prefersReducedMotion ? fadeIn : fadeUp}
          className="mt-12 flex w-full max-w-2xl justify-center"
        >
          <PromptBox />
        </motion.div>
      </motion.div>
      <motion.div
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ delay: 0.6, duration: 0.7 }}
        className="absolute bottom-10 z-10 flex flex-col items-center gap-2"
      >
        <span className="animate-float tracking-eyebrow font-mono text-xs text-ink-soft uppercase">
          {t.hero.scrollHint} ↓
        </span>
      </motion.div>
    </section>
  );
}
