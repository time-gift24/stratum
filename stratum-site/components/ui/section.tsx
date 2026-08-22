import { cn } from "@/lib/cn";
import type { ReactNode } from "react";

/** 段落外壳：中轴容器 + 垂直节奏，眉题 + 标题 + 内容。 */
type SectionProps = {
  id?: string;
  eyebrow?: string;
  /** 深处的段落加 on-abyss 反相文字与选区 */
  onAbyss?: boolean;
  className?: string;
  children: ReactNode;
};

export function Section({
  id,
  eyebrow,
  onAbyss = false,
  className,
  children,
}: SectionProps): ReactNode {
  return (
    <section
      id={id}
      className={cn(
        "py-section relative mx-auto w-full max-w-shell scroll-mt-20 px-6 sm:px-10",
        onAbyss && "on-abyss text-bone",
        className,
      )}
    >
      {eyebrow ? (
        <p
          className={cn(
            "tracking-eyebrow mb-8 font-mono text-xs font-medium uppercase",
            onAbyss ? "text-fog" : "text-ink",
          )}
        >
          {eyebrow}
        </p>
      ) : null}
      {children}
    </section>
  );
}

/** 段落大标题：中文黑体 + 英文衬线斜体强调词混排。 */
export function SectionTitle({
  titleA,
  accent,
  titleB,
  onAbyss = false,
  className,
}: {
  titleA: string;
  accent: string;
  titleB?: string;
  onAbyss?: boolean;
  className?: string;
}): ReactNode {
  return (
    <h2
      className={cn(
        "max-w-measure-title font-display text-headline font-bold tracking-display text-balance",
        onAbyss ? "text-bone" : "text-ink",
        className,
      )}
    >
      {titleA}
      <em className="font-serif font-normal italic">{accent}</em>
      {titleB}
    </h2>
  );
}
