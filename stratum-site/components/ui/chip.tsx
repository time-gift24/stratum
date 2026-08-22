import { cn } from "@/lib/cn";
import type { ReactNode } from "react";

/**
 * 状态 chip：等宽小字 + 状态点。
 * 状态点呼吸动画只属于"正在发生"，静止状态无色点（DESIGN.md）。
 */
const dots = {
  running: "bg-bronze animate-seal-pulse",
  approval: "bg-seal animate-seal-pulse",
} as const;

type ChipProps = {
  /** idle（静止）不渲染色点——DESIGN.md：呼吸与色点只属于"正在发生" */
  state?: keyof typeof dots | "idle";
  tone?: "paper" | "abyss";
  className?: string;
  children: ReactNode;
};

export function Chip({
  state = "idle",
  tone = "abyss",
  className,
  children,
}: ChipProps): ReactNode {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-2 rounded-pill px-4 py-2 font-mono text-xs tracking-label",
        tone === "abyss"
          ? "border border-bone/15 bg-bone/5 text-fog"
          : "border border-ink/15 bg-paper/60 text-ink-soft",
        className,
      )}
    >
      {state !== "idle" ? (
        <i
          aria-hidden
          className={cn("h-1.5 w-1.5 rounded-full", dots[state])}
        />
      ) : null}
      {children}
    </span>
  );
}
