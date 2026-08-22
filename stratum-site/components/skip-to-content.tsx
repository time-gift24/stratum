"use client";

/** Skip link：跳到 #main，文案跟随语言上下文。样式在 globals.css 的 .skip-to-content。 */
import { useLanguage } from "@/lib/i18n";
import type { ReactNode } from "react";

export function SkipToContent(): ReactNode {
  const { t } = useLanguage();
  return (
    <a href="#main" className="skip-to-content">
      {t.skipToContent}
    </a>
  );
}
