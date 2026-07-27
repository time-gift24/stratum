import type { ComponentPropsWithoutRef } from "react"

import { glassSurface } from "~/components/stratum/glass-surface"
import { cn } from "~/lib/utils"

export function FeatureCard({
  className,
  ...props
}: ComponentPropsWithoutRef<"div">) {
  return (
    <div
      data-slot="feature-card"
      className={cn(
        glassSurface({ surface: "card", elevation: "overlay" }),
        "group w-full rounded-xl p-3 transition-shadow duration-200 ease-out after:pointer-events-none after:absolute after:inset-x-4 after:top-0 after:h-24 after:bg-[radial-gradient(ellipse_11rem_5rem_at_76%_0%,color-mix(in_srgb,var(--chart-1)_34%,transparent),transparent_72%),radial-gradient(ellipse_12rem_5rem_at_48%_0%,color-mix(in_srgb,var(--primary)_22%,transparent),transparent_74%)] after:opacity-70 after:blur-xl hover:shadow-[0_36px_96px_color-mix(in_srgb,var(--background)_78%,transparent),0_16px_42px_color-mix(in_srgb,var(--primary)_6%,transparent),inset_0_1px_0_color-mix(in_srgb,var(--foreground)_12%,transparent)] motion-reduce:transition-none",
        className
      )}
      {...props}
    />
  )
}

export function FeatureCardContent({
  className,
  ...props
}: ComponentPropsWithoutRef<"div">) {
  return (
    <div
      data-slot="feature-card-content"
      className={cn(
        "rounded-xl bg-card/95 shadow-[0_24px_56px_color-mix(in_srgb,var(--background)_52%,transparent),inset_0_1px_0_color-mix(in_srgb,var(--foreground)_7%,transparent)]",
        className
      )}
      {...props}
    />
  )
}
