import { cva, type VariantProps } from "class-variance-authority"

export const glassSurface = cva(
  "relative isolate overflow-hidden border-0 backdrop-blur-2xl backdrop-saturate-125 before:pointer-events-none before:absolute before:inset-0 before:z-0 before:bg-[linear-gradient(145deg,color-mix(in_srgb,var(--foreground)_11%,transparent),transparent_40%,color-mix(in_srgb,var(--chart-5)_6%,transparent))] [&>*]:relative [&>*]:z-10",
  {
    variants: {
      surface: {
        popover: "bg-popover/62",
        card: "bg-card/70",
        sidebar: "bg-sidebar/68",
      },
      elevation: {
        navigation:
          "shadow-[0_26px_80px_color-mix(in_srgb,var(--background)_70%,transparent),0_10px_32px_color-mix(in_srgb,var(--chart-5)_5%,transparent),inset_0_1px_0_color-mix(in_srgb,var(--foreground)_10%,transparent)]",
        composer:
          "shadow-[0_32px_90px_color-mix(in_srgb,var(--background)_74%,transparent),0_14px_38px_color-mix(in_srgb,var(--primary)_7%,transparent),inset_0_1px_0_color-mix(in_srgb,var(--foreground)_11%,transparent)] transition-[box-shadow,transform,background-color] duration-200 ease-out focus-within:shadow-[0_36px_100px_color-mix(in_srgb,var(--background)_78%,transparent),0_18px_48px_color-mix(in_srgb,var(--primary)_12%,transparent),inset_0_1px_0_color-mix(in_srgb,var(--foreground)_15%,transparent)]",
        overlay:
          "shadow-[0_30px_84px_color-mix(in_srgb,var(--background)_74%,transparent),0_12px_34px_color-mix(in_srgb,var(--chart-5)_5%,transparent),inset_0_1px_0_color-mix(in_srgb,var(--foreground)_10%,transparent)]",
        dock: "shadow-[0_24px_68px_color-mix(in_srgb,var(--background)_72%,transparent),0_9px_28px_color-mix(in_srgb,var(--primary)_4%,transparent),inset_0_1px_0_color-mix(in_srgb,var(--sidebar-foreground)_10%,transparent)]",
        inset:
          "shadow-[inset_0_1px_0_color-mix(in_srgb,var(--foreground)_7%,transparent),inset_0_-1px_0_color-mix(in_srgb,var(--background)_26%,transparent),0_18px_48px_color-mix(in_srgb,var(--background)_38%,transparent)]",
      },
    },
    defaultVariants: {
      surface: "popover",
      elevation: "navigation",
    },
  }
)

export type GlassSurfaceVariants = VariantProps<typeof glassSurface>
