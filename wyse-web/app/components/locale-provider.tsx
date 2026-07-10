"use client"

import { createContext, useContext, useEffect, useState } from "react"

import {
  LOCALE_STORAGE_KEY,
  messages,
  resolveLocale,
  type Locale,
  type MessageKey,
} from "~/lib/locale"

type LocaleContextValue = {
  locale: Locale
  setLocale: (locale: Locale) => void
  t: (key: MessageKey) => string
}

const LocaleContext = createContext<LocaleContextValue | undefined>(undefined)

function applyLocale(locale: Locale) {
  document.documentElement.lang = locale === "zh" ? "zh-CN" : "en"
}

export function LocaleProvider({ children }: { children: React.ReactNode }) {
  const [locale, updateLocale] = useState<Locale>("zh")

  useEffect(() => {
    const resolvedLocale = resolveLocale(
      localStorage.getItem(LOCALE_STORAGE_KEY),
      navigator.language
    )
    updateLocale(resolvedLocale)
    applyLocale(resolvedLocale)
  }, [])

  const setLocale = (nextLocale: Locale) => {
    updateLocale(nextLocale)
    localStorage.setItem(LOCALE_STORAGE_KEY, nextLocale)
    applyLocale(nextLocale)
  }

  return (
    <LocaleContext.Provider
      value={{
        locale,
        setLocale,
        t: (key) => messages[locale][key],
      }}
    >
      {children}
    </LocaleContext.Provider>
  )
}

export function useLocale() {
  const context = useContext(LocaleContext)

  if (context === undefined) {
    throw new Error("useLocale must be used within a LocaleProvider")
  }

  return context
}
