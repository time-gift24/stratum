"use client";

import { Card } from "@/components/ui/card";
import { Section, SectionTitle } from "@/components/ui/section";
import { useLanguage } from "@/lib/i18n";
import { fadeIn, fadeUp, stagger, useReducedMotion } from "@/lib/motion";
import { motion } from "motion/react";
import type { ReactNode } from "react";

/** 雾中层：四个机制能力卡。 */
export function Mechanism(): ReactNode {
  const { t } = useLanguage();
  const prefersReducedMotion = useReducedMotion();

  return (
    <Section id="mechanism" eyebrow={t.mechanism.eyebrow}>
      <SectionTitle
        titleA={t.mechanism.titleA}
        accent={t.mechanism.titleAccent}
      />
      <motion.div
        initial="hidden"
        whileInView="visible"
        viewport={{ once: true, amount: 0.25 }}
        variants={prefersReducedMotion ? fadeIn : stagger}
        className="mt-14 grid gap-5 sm:grid-cols-2"
      >
        {t.mechanism.items.map((item) => (
          <motion.div
            key={item.title}
            variants={prefersReducedMotion ? fadeIn : fadeUp}
          >
            <Card className="h-full">
              <h3 className="text-lg font-bold">{item.title}</h3>
              <p className="leading-prose mt-3 text-ui text-ink-soft">
                {item.body}
              </p>
            </Card>
          </motion.div>
        ))}
      </motion.div>
    </Section>
  );
}
