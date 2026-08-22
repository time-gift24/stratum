import { cn } from "@/lib/cn";
import type { ComponentProps, ReactNode } from "react";

const tones = {
  /** 纸面卡片 */
  paper: "bg-paper-dim text-ink shadow-card hover:shadow-card-hover",
  /** 深处卡片 */
  abyss: "bg-abyss-raise text-bone shadow-card-deep hover:shadow-card-deep-hover",
} as const;

type CardProps = ComponentProps<"div"> & {
  tone?: keyof typeof tones;
};

export function Card({
  tone = "paper",
  className,
  children,
  ...props
}: CardProps): ReactNode {
  return (
    <div
      className={cn(
        "rounded-card p-6 transition-[background-color,box-shadow,transform] duration-250 ease-fluid hover:-translate-y-0.5 sm:p-8",
        tones[tone],
        className,
      )}
      {...props}
    >
      {children}
    </div>
  );
}
