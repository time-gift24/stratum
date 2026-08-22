"use client";

/**
 * 主对话区外壳：顶栏（展开动画）+ 侧栏 + 消息区 + 底部停靠 PromptBox。
 * Runtime 未接入本站点：落地页 ?task= 预填输入框，提交只在本地追加用户消息，
 * 并以 mono 小字明示预览状态——不伪造 agent 回复（PRODUCT.md 真实状态原则）。
 */
import { AppSidebar } from "@/components/app/app-sidebar";
import { AppTopBar } from "@/components/app/app-top-bar";
import { PromptBox } from "@/components/ui/prompt-box";
import { useLanguage } from "@/lib/i18n";
import { useSearchParams } from "next/navigation";
import { useState, type ReactNode } from "react";

export function ConversationShell(): ReactNode {
  const { t } = useLanguage();
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [messages, setMessages] = useState<string[]>([]);
  const taskParam = useSearchParams().get("task") ?? "";

  return (
    <div className="flex min-h-svh flex-col bg-paper">
      <AppTopBar onMenuClick={() => setSidebarOpen(true)} />
      <div className="flex w-full flex-1 pt-20">
        <AppSidebar open={sidebarOpen} onClose={() => setSidebarOpen(false)} />

        <main
          id="main"
          tabIndex={-1}
          className="flex min-w-0 flex-1 flex-col px-6 sm:px-10"
        >
          <div className="mx-auto flex w-full max-w-3xl flex-1 flex-col justify-center">
            {messages.length === 0 ? (
              <div className="pb-16 text-center">
                <h1 className="font-display text-headline font-bold tracking-display text-balance text-ink">
                  {t.conversation.emptyTitle}
                </h1>
                <p className="tracking-label mt-4 font-mono text-xs text-ink-soft uppercase">
                  {t.conversation.emptyNote}
                </p>
              </div>
            ) : (
              <div
                aria-live="polite"
                className="flex flex-col items-end gap-3 py-8"
              >
                {messages.map((m, i) => (
                  <p
                    key={i}
                    className="rounded-card bg-ink px-5 py-3 text-ui break-words text-paper"
                  >
                    {m}
                  </p>
                ))}
              </div>
            )}
          </div>

          <div className="sticky bottom-0 mx-auto flex w-full max-w-3xl justify-center pt-4 pb-[max(1.5rem,env(safe-area-inset-bottom))]">
            <PromptBox
              initialValue={taskParam}
              onSubmit={(task) => {
                if (task) setMessages((prev) => [...prev, task]);
              }}
            />
          </div>
        </main>
      </div>
    </div>
  );
}
