"use client";

import { cn } from "@/lib/cn";
import { useLanguage } from "@/lib/i18n";
import type { ReactNode } from "react";

/** 中 / EN 切换：当前语言钤印红底。 */
export function LanguageToggle(): ReactNode {
  const { lang, setLang } = useLanguage();

  return (
    <div
      role="group"
      aria-label="Language / 语言"
      className="tracking-label flex rounded-full border border-current/25 font-mono text-xs"
    >
      {(["zh", "en"] as const).map((l) => (
        <button
          key={l}
          type="button"
          onClick={() => setLang(l)}
          aria-pressed={lang === l}
          className={cn(
            "cursor-pointer rounded-full px-3.5 py-2.5 -my-1.5 transition-[background-color,color,opacity] duration-250",
            lang === l ? "bg-seal text-paper" : "opacity-75 hover:opacity-100",
          )}
        >
          {l === "zh" ? "中" : "EN"}
        </button>
      ))}
    </div>
  );
}
