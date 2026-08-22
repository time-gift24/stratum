import { cn } from "@/lib/cn";
import type { ComponentProps, ReactNode } from "react";

const base =
  "inline-flex cursor-pointer items-center justify-center gap-2 rounded-pill font-semibold transition-[background-color,color,box-shadow,transform] duration-250 ease-fluid hover:-translate-y-0.5 hover:shadow-button-hover active:translate-y-0 active:shadow-none";

const variants = {
  /** 墨地纸字，hover 钤印红 */
  primary: "bg-ink text-paper hover:bg-seal px-8 py-4 text-ui",
  /** 透明描边 */
  ghost: "border border-ink/25 text-ink hover:border-ink px-8 py-4 text-ui",
  /** 深处反相 */
  abyss: "bg-bone text-abyss hover:bg-seal hover:text-paper px-8 py-4 text-ui",
} as const;

export type ButtonVariant = keyof typeof variants;

type ButtonProps = ComponentProps<"button"> & {
  variant?: ButtonVariant;
};

export function Button({
  variant = "primary",
  className,
  children,
  ...props
}: ButtonProps): ReactNode {
  return (
    <button className={cn(base, variants[variant], className)} {...props}>
      {children}
    </button>
  );
}

type ButtonLinkProps = ComponentProps<"a"> & {
  variant?: ButtonVariant;
};

export function ButtonLink({
  variant = "primary",
  className,
  children,
  ...props
}: ButtonLinkProps): ReactNode {
  return (
    <a className={cn(base, variants[variant], className)} {...props}>
      {children}
    </a>
  );
}
