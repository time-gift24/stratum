"use client";

/**
 * 签名对话盒：多行自动长高输入 + 模型选择 + 工具开关 + 实时字数 + 发送/停止。
 * 交互骨架参照 React Bits Pro「Prompt Input 2」（Dropdown 已抽为公共组件），
 * 样式全部消费纸墨 token；少边框纪律——聚焦只加深阴影。
 * 默认提交跳对话页；宿主可用 onSubmit 接管（对话页本地追加）。
 */
import { Dropdown } from "@/components/ui/dropdown";
import { siteConfig } from "@/lib/config";
import { useLanguage } from "@/lib/i18n";
import { cn } from "@/lib/cn";
import { ArrowUp, Globe, Sparkles, Square } from "lucide-react";
import { useRouter } from "next/navigation";
import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
  type ReactNode,
} from "react";

const MAX_COMPOSER_HEIGHT = 200;

function resizeTextarea(el: HTMLTextAreaElement) {
  el.style.height = "auto";
  if (el.scrollHeight === 0) return;
  el.style.height = `${Math.min(el.scrollHeight, MAX_COMPOSER_HEIGHT)}px`;
}

type PromptBoxProps = {
  /** 初始内容（如落地页带过来的 ?task=） */
  initialValue?: string;
  /** 覆盖默认提交行为（默认跳转对话页）；对话页内提交走本地回调 */
  onSubmit?: (task: string) => void;
  /** 宿主驱动的执行态：true 时发送钮变停止钮 */
  busy?: boolean;
  onCancel?: () => void;
};

export function PromptBox({
  initialValue = "",
  onSubmit,
  busy = false,
  onCancel,
}: PromptBoxProps = {}): ReactNode {
  const { t } = useLanguage();
  const router = useRouter();
  const [value, setValue] = useState(initialValue);
  const [model, setModel] = useState(t.conversation.models[0].name);
  const [searchOn, setSearchOn] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // 自动长高：宽度/字体变化监听只挂一次，封顶 MAX_COMPOSER_HEIGHT
  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;

    el.ownerDocument.fonts?.ready
      .then(() => resizeTextarea(el))
      .catch(() => {});

    let lastWidth = el.clientWidth;
    const observer = new ResizeObserver(() => {
      if (el.clientWidth === lastWidth) return;
      lastWidth = el.clientWidth;
      resizeTextarea(el);
    });
    observer.observe(el);

    return () => observer.disconnect();
  }, []);

  // 内容变化时重算高度
  useEffect(() => {
    const el = textareaRef.current;
    if (el) resizeTextarea(el);
  }, [value]);

  const canSend = value.trim().length > 0 && !busy;

  function submit(task: string) {
    if (!task) return;
    if (onSubmit) {
      onSubmit(task);
      return;
    }
    router.push(
      `${siteConfig.conversationPath}?task=${encodeURIComponent(task)}`,
    );
  }

  function handleSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    if (!canSend) return;
    submit(value.trim());
    setValue("");
  }

  function onKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      if (canSend) {
        submit(value.trim());
        setValue("");
      }
    }
  }

  const charCount = value.trim().length;

  return (
    <form
      onSubmit={handleSubmit}
      className="rounded-card shadow-pill focus-within:shadow-pill-focus w-full max-w-2xl bg-box/85 p-3 backdrop-blur-md transition-shadow duration-250"
    >
      <label htmlFor="task-input" className="sr-only">
        {t.hero.pillPlaceholder}
      </label>
      <textarea
        ref={textareaRef}
        id="task-input"
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={onKeyDown}
        placeholder={t.hero.pillPlaceholder}
        rows={1}
        className="text-input text-ink placeholder:text-ink-soft/70 block max-h-50 min-h-13 w-full resize-none bg-transparent px-3 py-2.5 leading-relaxed outline-none"
      />

      <div className="flex items-center gap-2 px-1.5 pt-1 pb-1.5">
        <Dropdown
          label={t.conversation.modelLabel}
          options={t.conversation.models}
          value={model}
          onChange={setModel}
          triggerMaxWidth="max-w-36"
        />

        <button
          type="button"
          aria-pressed={searchOn}
          onClick={() => setSearchOn((v) => !v)}
          className={cn(
            "rounded-pill shadow-chip hover:shadow-chip-hover inline-flex h-9 shrink-0 cursor-pointer items-center gap-1.5 px-3.5 text-sm font-medium whitespace-nowrap transition-[background-color,color,box-shadow,transform] duration-250 ease-fluid active:scale-97",
            searchOn
              ? "bg-seal/8 text-seal"
              : "bg-white text-ink-soft hover:text-ink",
          )}
        >
          <Globe size={14} aria-hidden />
          {t.conversation.searchToggle}
        </button>

        <Dropdown
          label={t.conversation.suggestionsLabel}
          options={t.hero.suggestions.map((s) => ({ name: s }))}
          value=""
          onChange={(next) => setValue(next)}
          icon={<Sparkles size={14} aria-hidden />}
        />

        <div className="ml-auto flex items-center gap-2.5">
          <span className="min-w-14 text-right font-mono text-xs text-ink-soft tabular-nums">
            {charCount > 0 ? `${charCount} ${t.conversation.charsUnit}` : ""}
          </span>
          {busy ? (
            <button
              type="button"
              onClick={onCancel}
              aria-label={t.conversation.stop}
              className="rounded-pill flex h-11 w-11 shrink-0 cursor-pointer items-center justify-center bg-seal text-paper transition-[background-color,transform] duration-250 ease-fluid hover:bg-seal-deep active:scale-95"
            >
              <Square size={15} aria-hidden />
            </button>
          ) : (
            <button
              type="submit"
              aria-label={t.conversation.send}
              disabled={!canSend}
              className="rounded-pill bg-seal text-paper duration-250 ease-fluid hover:bg-seal-deep flex h-11 w-11 shrink-0 cursor-pointer items-center justify-center transition-[background-color,transform] hover:-translate-y-0.5 active:scale-95 disabled:pointer-events-none disabled:bg-paper-dim disabled:text-ink-soft/50"
            >
              <ArrowUp size={18} strokeWidth={2.2} aria-hidden />
            </button>
          )}
        </div>
      </div>
    </form>
  );
}
