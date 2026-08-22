"use client";

/**
 * 双语（中/英）上下文：默认跟随浏览器语言，手动切换后写入 localStorage。
 * 文案字典在 lib/content.ts，类型从字典推导，缺 key 直接编译错误。
 */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { content, type Content } from "@/lib/content";

export type Language = keyof Content;

const STORAGE_KEY = "stratum-site-lang";

function detectLanguage(): Language {
  if (typeof navigator === "undefined") return "zh";
  return navigator.language.toLowerCase().startsWith("zh") ? "zh" : "en";
}

type LanguageContextValue = {
  lang: Language;
  t: Content[Language];
  setLang: (lang: Language) => void;
};

const LanguageContext = createContext<LanguageContextValue | null>(null);

export function LanguageProvider({ children }: { children: ReactNode }): ReactNode {
  const [lang, setLangState] = useState<Language>("zh");

  useEffect(() => {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    const next = stored === "zh" || stored === "en" ? stored : detectLanguage();
    document.documentElement.lang = next === "zh" ? "zh-CN" : "en";
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 挂载后从 localStorage/浏览器语言纠正初始值，避免 SSR 水合不一致
    setLangState(next);
  }, []);

  const setLang = useCallback((next: Language) => {
    setLangState(next);
    window.localStorage.setItem(STORAGE_KEY, next);
    document.documentElement.lang = next === "zh" ? "zh-CN" : "en";
  }, []);

  const value = useMemo(
    () => ({ lang, t: content[lang], setLang }),
    [lang, setLang],
  );

  return (
    <LanguageContext.Provider value={value}>{children}</LanguageContext.Provider>
  );
}

export function useLanguage(): LanguageContextValue {
  const ctx = useContext(LanguageContext);
  if (!ctx) throw new Error("useLanguage must be used within LanguageProvider");
  return ctx;
}
