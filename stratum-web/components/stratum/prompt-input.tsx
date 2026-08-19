"use client"

import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react"
import { ArrowUp, Loader2, Plus, Square } from "lucide-react"

import { BorderGlow } from "@/components/react-bits/border-glow"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

/** textarea 高度上限（约 6-7 行），超过后内部滚动 */
const MAX_TEXTAREA_HEIGHT = 160
/** 框体圆角（多行后不再用 pill，单行/多行视觉一致） */
const CORNER_RADIUS = 28
/** 单行 ⇄ 多行布局判定的迟滞带（px）：进入多行 > 单行高 + 2，切回 ≤ 单行高 */
const MULTILINE_HYSTERESIS = 2
const subscribeToTheme = (onStoreChange: () => void) => {
  const observer = new MutationObserver(onStoreChange)
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["class"],
  })
  return () => observer.disconnect()
}
const getClientThemeSnapshot = () =>
  document.documentElement.classList.contains("dark")
const getServerThemeSnapshot = () => false

// SSR 下退化为 useEffect（layout 时机重测，避免翻转后闪一帧旧高度）
const useIsomorphicLayoutEffect =
  typeof window !== "undefined" ? useLayoutEffect : useEffect

/**
 * PromptInput —— Gemini 式提示词输入框（多行自适应 + 双结构）。
 * 单行：横向药丸——左侧 + 按钮 / 中间自动生长的 textarea / 右侧 trailing
 * 插槽与发送（items-center 垂直居中）。多行（scrollHeight 超单行高 + 2px 迟滞
 * 判定）：textarea 独占顶部整行，下方控制行（左 + 按钮，右 trailing + 发送，
 * justify-between）；删回单行立即切回。单一 DOM 顺序 + flex-wrap 实现，
 * textarea 不 remount、不丢焦点。multiline 的变更只来自输入事件（含窄形态
 * 退出预判）；形态翻转后按新宽度重测高度（layout 时机，只调高度不重判）。
 * 默认 1 行高，换行/长文本自动生长（scrollHeight 手法），超过 10rem 内部滚动。
 * Enter 提交、Shift+Enter 换行、IME 组合态 Enter 不提交；空输入禁用发送。
 * 执行中（running）发送钮变为停止钮，取消已发出（cancelRequested）则转圈
 * 禁用，等待真实停止。左侧插槽 leading 缺省时渲染默认 + 附件按钮。
 * 激活态：light 是 border 变色 + 贴边 ring（无 offset，单线光晕）；dark 聚焦
 * 时保留 BorderGlow 全线段点亮。popover portal 的焦点仍视为输入框内部交互，
 * 避免错误熄灭反馈。
 * 值默认内部自管（提交后清空），也可传 value/onChange 受控。
 */
export function PromptInput({
  placeholder = "问问 Stratum",
  leading,
  trailing,
  running = false,
  cancelRequested = false,
  onCancel,
  value,
  onChange,
  onSubmit,
  className,
}: {
  placeholder?: string
  /** 输入框左侧插槽（如 Agent 选择器）；不传则渲染默认的 + 附件按钮 */
  leading?: React.ReactNode
  /** 输入框右侧、发送按钮之前的插槽（如模型选择器）；不传则不渲染 */
  trailing?: React.ReactNode
  /** 执行中：发送钮变为停止钮，点击走 onCancel */
  running?: boolean
  /** 取消请求已发出：停止钮转圈并禁用，等待真实停止 */
  cancelRequested?: boolean
  onCancel?: () => void
  /** 受控值；不传则内部自管（提交后自动清空） */
  value?: string
  onChange?: (value: string) => void
  onSubmit?: (value: string) => void
  className?: string
}) {
  const [innerValue, setInnerValue] = useState("")
  const dark = useSyncExternalStore(
    subscribeToTheme,
    getClientThemeSnapshot,
    getServerThemeSnapshot
  )
  const controlled = value !== undefined
  const currentValue = controlled ? value : innerValue
  const [focused, setFocused] = useState(false)
  const [multiline, setMultiline] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  // 单行内容高度（line-height + 上下 padding），首次测量缓存
  const singleLineHeightRef = useRef(0)
  // onChange 已测过的值：effect 跳过重复测量，避免每键双 reflow
  const measuredValueRef = useRef<string | null>(null)
  const canSend = currentValue.trim().length > 0

  const measureSingleLineHeight = (
    textarea: HTMLTextAreaElement,
    scrollHeight: number
  ) => {
    if (singleLineHeightRef.current !== 0) return
    const style = getComputedStyle(textarea)
    const lineHeight = parseFloat(style.lineHeight)
    // computed line-height 可能是 "normal"（NaN）：回退用首次 scrollHeight 作基准
    singleLineHeightRef.current = Number.isFinite(lineHeight)
      ? lineHeight +
        parseFloat(style.paddingTop) +
        parseFloat(style.paddingBottom)
      : scrollHeight
  }

  const syncHeight = (textarea: HTMLTextAreaElement) => {
    textarea.style.height = "auto"
    textarea.style.height = `${Math.min(textarea.scrollHeight, MAX_TEXTAREA_HEIGHT)}px`
  }

  // resize + multiline 判定都在事件里做（onChange/提交校准），不在 effect 里
  // 判 multiline——effect 依赖 multiline 时，翻转改变 textarea 宽度、scrollHeight
  // 测量基准随之改变，边界宽度会来回翻转形成微任务死循环（页面冻结的根因）
  const measure = (textarea: HTMLTextAreaElement) => {
    syncHeight(textarea)
    const scrollHeight = textarea.scrollHeight
    measuredValueRef.current = textarea.value
    measureSingleLineHeight(textarea, scrollHeight)

    const single = singleLineHeightRef.current
    let next = multiline
      ? scrollHeight > single
      : scrollHeight > single + MULTILINE_HYSTERESIS
    if (multiline && !next) {
      // 退出预判：宽形态一行高的文本在窄（单行）形态可能折两行——临时套用
      // 窄形态量一次（同步在事件内，无循环），边界内容保持多行直到真正单行
      const previousOrder = textarea.style.order
      const previousBasis = textarea.style.flexBasis
      textarea.style.order = "0"
      textarea.style.flexBasis = "0%"
      textarea.style.height = "auto"
      next = textarea.scrollHeight > single
      textarea.style.order = previousOrder
      textarea.style.flexBasis = previousBasis
      syncHeight(textarea)
    }
    if (next !== multiline) setMultiline(next)
  }

  // 形态翻转后按新宽度重测高度（layout 时机，不闪帧）。只调高度、不重判
  // multiline——multiline 的变更永远只来自输入事件，因此无"测量→翻转→测量"循环
  useIsomorphicLayoutEffect(() => {
    const textarea = textareaRef.current
    if (textarea) syncHeight(textarea)
  }, [multiline])

  // 轻量兜底：onChange 已测的值直接跳过；仅外部值变化（受控灌入/清空、
  // 挂载）时对齐高度并补做迟滞判定。函数式更新：值不变返回 prev 不触发渲染；
  // 异步调度避开 effect 内同步 setState（react-hooks/set-state-in-effect）
  useEffect(() => {
    const textarea = textareaRef.current
    if (!textarea || measuredValueRef.current === currentValue) return
    textarea.style.height = "auto"
    const scrollHeight = textarea.scrollHeight
    textarea.style.height = `${Math.min(scrollHeight, MAX_TEXTAREA_HEIGHT)}px`
    measuredValueRef.current = currentValue
    measureSingleLineHeight(textarea, scrollHeight)

    const single = singleLineHeightRef.current
    void Promise.resolve().then(() =>
      setMultiline((prev) => {
        const next = prev
          ? scrollHeight > single
          : scrollHeight > single + MULTILINE_HYSTERESIS
        return next === prev ? prev : next
      })
    )
  }, [currentValue])

  const updateValue = (next: string) => {
    if (controlled) {
      onChange?.(next)
    } else {
      setInnerValue(next)
    }
  }

  const submit = () => {
    if (!canSend) return
    onSubmit?.(currentValue.trim())
    if (!controlled) setInnerValue("")
    // 提交后回到单行结构（高度由上面的 effect 回收）
    setMultiline(false)
  }

  const composer = (
    <div className="flex flex-wrap items-center justify-between gap-1.5 rounded-[28px] p-1.5 shadow-sm dark:shadow-xl">
      {leading ?? (
        <Button
          variant="ghost"
          size="icon"
          className="size-11 rounded-full sm:size-7"
          aria-label="添加附件"
        >
          <Plus aria-hidden />
        </Button>
      )}
      <textarea
        ref={textareaRef}
        rows={1}
        value={currentValue}
        onChange={(e) => {
          updateValue(e.target.value)
          measure(e.target)
        }}
        onKeyDown={(e) => {
          // Enter 提交、Shift+Enter 换行；中文/日文输入法下 Enter 是确认候选词
          if (e.key !== "Enter" || e.shiftKey) return
          if (e.nativeEvent.isComposing) return
          e.preventDefault()
          submit()
        }}
        placeholder={placeholder}
        aria-label={placeholder}
        style={{ maxHeight: MAX_TEXTAREA_HEIGHT }}
        className={cn(
          "min-w-0 flex-1 resize-none overflow-y-auto bg-transparent px-1 py-2.5 font-sans text-base leading-6 text-foreground outline-none placeholder:text-muted-foreground",
          multiline && "order-first basis-full"
        )}
      />
      <div className="flex items-center gap-1.5">
        {trailing}
        {running ? (
          <Button
            size="icon"
            className="size-11 rounded-full sm:size-7"
            aria-label={cancelRequested ? "正在取消" : "取消执行"}
            disabled={cancelRequested}
            onClick={onCancel}
          >
            {cancelRequested ? (
              <Loader2
                aria-hidden
                className="animate-spin motion-reduce:animate-none"
              />
            ) : (
              <Square aria-hidden fill="currentColor" />
            )}
          </Button>
        ) : (
          <Button
            size="icon"
            className="size-11 rounded-full sm:size-7"
            aria-label="发送"
            disabled={!canSend}
            onClick={submit}
          >
            <ArrowUp aria-hidden />
          </Button>
        )}
      </div>
    </div>
  )

  return (
    <div
      data-slot="prompt-input"
      className={cn("w-full", className)}
      onFocus={() => setFocused(true)}
      onBlur={() => {
        // popover（如 ModelSelector 内容）portal 到 body，relatedTarget 判断
        // 必然失效；延迟一帧看真实焦点落点——在组件内或任一 popover 内都保持点亮
        requestAnimationFrame(() => {
          const active = document.activeElement
          if (active instanceof HTMLElement) {
            if (rootRef.current?.contains(active)) return
            if (active.closest('[data-slot="popover-content"]')) return
          }
          setFocused(false)
        })
      }}
      ref={rootRef}
    >
      {/* 单一 DOM 顺序 + flex-wrap：单行横排，多行 textarea 独占首行。 */}
      {dark ? (
        <BorderGlow
          active={focused}
          flat
          borderRadius={CORNER_RADIUS}
          glowRadius={28}
          className="rounded-[28px]"
        >
          {composer}
        </BorderGlow>
      ) : (
        <div className="grid rounded-[28px] border border-border bg-card transition-[border-color,box-shadow] focus-within:border-ring focus-within:ring-2 focus-within:ring-ring/30">
          {composer}
        </div>
      )}
    </div>
  )
}
