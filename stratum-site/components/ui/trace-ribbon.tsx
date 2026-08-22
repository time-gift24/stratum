"use client";

/**
 * 执行轨迹带：一个 turn 的磷青轨迹 + 琥珀审批尖峰。
 * 进入视口时轨迹自左向右绘出一次；reduced-motion 直接呈现完整轨迹。
 */
import { useReducedMotion } from "@/lib/motion";
import { motion } from "motion/react";
import type { ReactNode } from "react";

const TRACE_PATH =
  "M0 60 L180 60 L200 30 L260 30 L280 84 L340 84 L360 60 L520 60 L545 14 L575 96 L600 44 L640 60 L1080 60";

export function TraceRibbon(): ReactNode {
  const prefersReducedMotion = useReducedMotion();

  return (
    <svg
      viewBox="0 0 1080 120"
      fill="none"
      aria-hidden
      className="h-auto w-full"
    >
      <defs>
        <linearGradient id="trace-stroke" x1="0" x2="1">
          <stop offset="0" stopColor="var(--color-bronze)" stopOpacity="0.1" />
          <stop offset="0.25" stopColor="var(--color-bronze)" />
          <stop offset="0.7" stopColor="var(--color-bone)" />
          <stop offset="1" stopColor="var(--color-bronze)" stopOpacity="0.25" />
        </linearGradient>
        <linearGradient id="trace-area" x1="0" x2="0" y1="0" y2="1">
          <stop offset="0" stopColor="var(--color-bronze)" stopOpacity="0.22" />
          <stop offset="1" stopColor="var(--color-bronze)" stopOpacity="0" />
        </linearGradient>
      </defs>
      <path
        d={`${TRACE_PATH} L1080 120 L0 120 Z`}
        fill="url(#trace-area)"
      />
      <motion.path
        d={TRACE_PATH}
        stroke="url(#trace-stroke)"
        strokeWidth="2.5"
        initial={prefersReducedMotion ? false : { pathLength: 0 }}
        whileInView={{ pathLength: 1 }}
        viewport={{ once: true, amount: 0.6 }}
        transition={{ duration: 1.6, ease: [0.22, 1, 0.36, 1] }}
      />
      <circle cx="545" cy="14" r="4" fill="var(--color-seal)" />
      <text
        x="556"
        y="16"
        fill="var(--color-seal)"
        fontSize="12"
        className="font-mono"
        letterSpacing="2"
      >
        APPROVAL
      </text>
      <circle cx="200" cy="30" r="3.5" fill="var(--color-bone)" />
      <circle cx="280" cy="84" r="3.5" fill="var(--color-bone)" />
    </svg>
  );
}
