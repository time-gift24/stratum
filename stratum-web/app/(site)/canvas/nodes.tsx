import { ChevronDown, Dices, Minus, Plus } from "lucide-react"

import { NodePort } from "@/components/stratum/workflow-node"

/**
 * 画布演示节点的主体内容（静态演示数据，见 .impeccable/reference/workflow-editor.png）。
 * 端口通过 port prop 注册为锚点，WorkflowGraph 据此自动连线。
 */

export function ModelNodeBody() {
  return (
    <>
      <div className="flex flex-col items-end gap-1">
        <NodePort port="model" tone="model" align="end">
          model
        </NodePort>
        <NodePort port="positive" tone="positive" align="end">
          positive
        </NodePort>
        <NodePort port="negative" tone="negative" align="end">
          negative
        </NodePort>
      </div>
      <button
        type="button"
        className="flex w-full items-center justify-between rounded-lg bg-background px-2.5 py-2 text-left font-sans text-xs"
      >
        DreamShaper 6 (SD1.5)
        <ChevronDown aria-hidden className="size-3 text-muted-foreground" />
      </button>
    </>
  )
}

export function PositivePromptBody() {
  return (
    <>
      <p className="font-sans text-xs leading-relaxed">
        A black bear with a pink snout, minimalist style, soft gradients, clear
        blue sky
      </p>
      <textarea
        aria-label="正向提示词"
        placeholder="Type what you want to get"
        rows={1}
        wrap="off"
        className="w-full resize-none overflow-hidden rounded-lg bg-background px-2.5 py-2 font-sans text-xs text-muted-foreground outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring/30"
      />
    </>
  )
}

export function NegativePromptBody() {
  return (
    <>
      <p className="font-sans text-xs leading-relaxed text-muted-foreground">
        No text, unnecessary details, background objects, other animals or
        people
      </p>
      <textarea
        aria-label="负向提示词"
        placeholder="Type what do not you want to get"
        rows={1}
        wrap="off"
        className="w-full resize-none overflow-hidden rounded-lg bg-background px-2.5 py-2 font-sans text-xs text-muted-foreground outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring/30"
      />
    </>
  )
}

const fieldControlClass =
  "flex h-6 items-center gap-1 rounded-md bg-background px-1.5 font-sans text-[0.625rem]"

function FieldRow({
  label,
  children,
}: {
  label: string
  children: React.ReactNode
}) {
  return (
    <div className="flex items-center justify-between gap-2">
      <span className="font-sans text-[0.625rem] text-muted-foreground">
        {label}
      </span>
      {children}
    </div>
  )
}

export function GeneratorNodeBody() {
  return (
    <>
      <div className="flex items-start justify-between">
        <div className="flex flex-col gap-1">
          <NodePort port="model" tone="model">
            model
          </NodePort>
          <NodePort port="positive" tone="positive">
            positive
          </NodePort>
          <NodePort port="negative" tone="negative">
            negative
          </NodePort>
        </div>
        <NodePort port="image" tone="image" align="end">
          image
        </NodePort>
      </div>
      <div className="flex flex-col gap-1.5">
        <FieldRow label="Randomness">
          <span className={fieldControlClass}>
            12345
            <Dices aria-hidden className="size-3 text-muted-foreground" />
          </span>
        </FieldRow>
        <FieldRow label="Control mode">
          <span className={fieldControlClass}>
            <span className="rounded-sm bg-port-image px-1.5 py-0.5 text-background">
              Fixed
            </span>
          </span>
        </FieldRow>
        <FieldRow label="Quality steps">
          <span className={fieldControlClass}>
            <Minus aria-hidden className="size-3 text-muted-foreground" />
            30
            <Plus aria-hidden className="size-3 text-muted-foreground" />
          </span>
        </FieldRow>
        <FieldRow label="Prompt strength">
          <span className={fieldControlClass}>
            8.0
            <ChevronDown aria-hidden className="size-3 text-muted-foreground" />
          </span>
        </FieldRow>
        <FieldRow label="Sampling method">
          <span className={fieldControlClass}>
            dpm++ 2M
            <ChevronDown aria-hidden className="size-3 text-muted-foreground" />
          </span>
        </FieldRow>
      </div>
    </>
  )
}

export function PreviewNodeBody() {
  return (
    <>
      <NodePort port="image" tone="image">
        image
      </NodePort>
      <figure className="relative overflow-hidden rounded-xl">
        {/* 演示数据：无真实生成结果，用多层渐变 + 熊形剪影合成占位插画 */}
        <div
          aria-hidden
          className="aspect-[4/5] w-full"
          style={{
            background: [
              "radial-gradient(ellipse 55% 38% at 62% 22%, oklch(0.82 0.12 15 / 95%), transparent 70%)",
              "radial-gradient(ellipse 60% 45% at 40% 78%, oklch(0.62 0.14 250 / 85%), transparent 72%)",
              "radial-gradient(ellipse 45% 40% at 68% 60%, oklch(0.9 0.05 200 / 60%), transparent 70%)",
              "linear-gradient(180deg, oklch(0.22 0.04 250), oklch(0.16 0.03 260))",
            ].join(", "),
          }}
        />
        <svg
          aria-hidden
          viewBox="0 0 200 250"
          className="absolute inset-0 size-full"
        >
          <defs>
            <filter
              id="bear-soft"
              x="-30%"
              y="-30%"
              width="160%"
              height="160%"
            >
              <feGaussianBlur stdDeviation="7" />
            </filter>
          </defs>
          <g filter="url(#bear-soft)" fill="oklch(0.94 0.03 200 / 55%)">
            <circle cx="68" cy="72" r="24" />
            <circle cx="138" cy="64" r="26" />
            <ellipse cx="103" cy="128" rx="60" ry="56" />
          </g>
          <ellipse
            cx="118"
            cy="142"
            rx="26"
            ry="18"
            fill="oklch(0.88 0.08 15 / 45%)"
            filter="url(#bear-soft)"
          />
        </svg>
        <figcaption className="absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/70 to-transparent px-3 pt-8 pb-2.5">
          <p className="font-sans text-xs font-medium text-white">
            Final Result
          </p>
          <p className="mt-0.5 line-clamp-2 font-sans text-[0.625rem] leading-relaxed text-white/70">
            Minimalist illustration of a black bear with a pink snout, soft
            gradients, and smooth shapes, against a clear blue sky
          </p>
        </figcaption>
      </figure>
    </>
  )
}
