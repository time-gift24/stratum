import type { ComponentProps } from "react"

import compactStratumMarkSvg from "~/assets/stratum-mark-compact.svg?raw"
import stratumMarkSvg from "~/assets/stratum-mark.svg?raw"
import { cn } from "~/lib/utils"

type StratumMarkProps = Omit<
  ComponentProps<"span">,
  "children" | "dangerouslySetInnerHTML"
> & {
  variant?: "default" | "compact"
}

export function StratumMark({
  className,
  variant = "default",
  ...props
}: StratumMarkProps) {
  const svgMarkup =
    variant === "compact" ? compactStratumMarkSvg : stratumMarkSvg

  return (
    <span
      aria-hidden="true"
      {...props}
      className={cn(
        "inline-block shrink-0 leading-none [&>svg]:block [&>svg]:size-full [&>svg]:overflow-visible",
        variant === "compact" && "stratum-mark--compact",
        className
      )}
      dangerouslySetInnerHTML={{ __html: svgMarkup }}
    />
  )
}
