import type { ReactNode } from "react"

import { BrainIcon, ChevronDownIcon } from "lucide-react"

import { cn } from "~/lib/utils"

export function AiReasoning({
  children,
  streaming = false,
  completeLabel,
  thinkingLabel,
}: {
  children: ReactNode
  streaming?: boolean
  completeLabel: string
  thinkingLabel: string
}) {
  return (
    <details
      data-slot="reasoning"
      data-state={streaming ? "streaming" : "complete"}
      open={streaming}
      className="group/ai-reasoning mt-2 w-full max-w-[44rem] rounded-md border border-border/60 bg-muted/35 text-xs/relaxed text-muted-foreground"
    >
      <summary className="flex cursor-pointer list-none items-center gap-2 px-3 py-2 font-medium text-foreground marker:hidden">
        <BrainIcon aria-hidden="true" className="size-3.5 text-muted-foreground" />
        <span className={cn(streaming && "text-primary")}>
          {streaming ? thinkingLabel : completeLabel}
        </span>
        <ChevronDownIcon
          aria-hidden="true"
          className="ml-auto size-3.5 text-muted-foreground transition-transform group-open/ai-reasoning:rotate-180"
        />
      </summary>
      <div
        data-slot="reasoning-content"
        className="border-t border-border/50 px-3 py-2 whitespace-pre-wrap"
      >
        {children}
      </div>
    </details>
  )
}
