import * as React from "react"

import { cn } from "@/lib/utils"

/**
 * TelemetryReadout —— 画布角落的遥测读数：等宽小字逐行排列（T / I / N / S）。
 */
function TelemetryReadout({
  items,
  className,
  ...props
}: React.ComponentProps<"dl"> & { items: { label: string; value: string }[] }) {
  return (
    <dl
      data-slot="telemetry-readout"
      className={cn(
        "font-mono text-[0.625rem] leading-relaxed text-muted-foreground/70",
        className
      )}
      {...props}
    >
      {items.map((item) => (
        <div key={item.label} className="flex gap-1.5">
          <dt>{item.label}:</dt>
          <dd>{item.value}</dd>
        </div>
      ))}
    </dl>
  )
}

export { TelemetryReadout }
