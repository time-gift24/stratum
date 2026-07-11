import assert from "node:assert/strict"
import { existsSync, readFileSync } from "node:fs"
import test from "node:test"

const localePaths = {
  en: new URL("../locales/en.json", import.meta.url),
  zh: new URL("../locales/zh.json", import.meta.url),
}

function flattenKeys(value, prefix = "") {
  return Object.entries(value).flatMap(([key, child]) => {
    const path = prefix ? `${prefix}.${key}` : key
    return typeof child === "object" && child !== null
      ? flattenKeys(child, path)
      : path
  })
}

test("English and Chinese locale resources exist with matching keys", () => {
  for (const [language, path] of Object.entries(localePaths)) {
    assert.equal(
      existsSync(path),
      true,
      `${language} locale resource is missing`
    )
  }

  const en = JSON.parse(readFileSync(localePaths.en, "utf8"))
  const zh = JSON.parse(readFileSync(localePaths.zh, "utf8"))

  assert.deepEqual(flattenKeys(zh).sort(), flattenKeys(en).sort())
})

test("design tokens keep structural corners restrained", () => {
  const css = readFileSync(new URL("../app.css", import.meta.url), "utf8")
  const navbar = readFileSync(
    new URL("../components/site-navbar.tsx", import.meta.url),
    "utf8"
  )

  assert.match(css, /--radius: 0\.5rem;/)
  assert.match(css, /--radius-wyse-panel: 0\.75rem;/)
  assert.match(css, /--radius-wyse-shell: 1rem;/)
  assert.doesNotMatch(navbar, /borderRadius=\{999\}/)
})

test("request language prefers the saved choice over the browser language", async () => {
  const modulePath = new URL("./locale.ts", import.meta.url)
  assert.equal(existsSync(modulePath), true, "locale module is missing")

  const { getRequestLanguage, serializeLanguageCookie } = await import(
    modulePath.href
  )
  const savedChinese = new Request("https://example.test", {
    headers: {
      "accept-language": "en-US,en;q=0.9",
      cookie: "wyse-language=zh",
    },
  })
  const chineseBrowser = new Request("https://example.test", {
    headers: { "accept-language": "zh-CN,zh;q=0.9,en;q=0.8" },
  })
  const englishBrowser = new Request("https://example.test", {
    headers: { "accept-language": "en-US,en;q=0.9" },
  })
  const savedEnglish = new Request("https://example.test", {
    headers: { cookie: "wyse-language=en" },
  })

  assert.equal(getRequestLanguage(savedChinese), "zh")
  assert.equal(getRequestLanguage(chineseBrowser), "zh")
  assert.equal(getRequestLanguage(englishBrowser), "zh")
  assert.equal(getRequestLanguage(savedEnglish), "en")
  assert.equal(getRequestLanguage(new Request("https://example.test")), "zh")
  assert.equal(
    serializeLanguageCookie("en"),
    "wyse-language=en; Path=/; Max-Age=31536000; SameSite=Lax"
  )
})

test("each request gets an isolated i18next instance", async () => {
  const modulePath = new URL("./i18n.ts", import.meta.url)
  assert.equal(existsSync(modulePath), true, "i18next setup is missing")

  const { createI18n } = await import(modulePath.href)
  const english = createI18n("en")
  const chinese = createI18n("zh")

  assert.notEqual(english, chinese)
  assert.equal(english.t("hero.title"), "Build typed agents")
  assert.equal(chinese.t("hero.title"), "构建强类型智能体")
})

test("the app shell and visible controls are wired to i18next", () => {
  const root = readFileSync(new URL("../root.tsx", import.meta.url), "utf8")
  const home = readFileSync(
    new URL("../routes/home.tsx", import.meta.url),
    "utf8"
  )
  const navbar = readFileSync(
    new URL("../components/site-navbar.tsx", import.meta.url),
    "utf8"
  )
  const themeToggle = readFileSync(
    new URL("../components/theme-toggle.tsx", import.meta.url),
    "utf8"
  )
  const languageTogglePath = new URL(
    "../components/language-toggle.tsx",
    import.meta.url
  )

  assert.equal(
    existsSync(languageTogglePath),
    true,
    "language toggle is missing"
  )
  const languageToggle = readFileSync(languageTogglePath, "utf8")

  assert.match(root, /I18nextProvider/)
  assert.match(root, /getRequestLanguage/)
  assert.match(home, /useTranslation/)
  assert.match(navbar, /useTranslation/)
  assert.match(themeToggle, /useTranslation/)
  assert.match(languageToggle, /changeLanguage/)
  assert.match(languageToggle, /localStorage\.setItem/)
  assert.match(languageToggle, /document\.cookie/)
})

test("the navbar keeps one Chinese brand and aligns desktop navigation right", () => {
  const navbar = readFileSync(
    new URL("../components/site-navbar.tsx", import.meta.url),
    "utf8"
  )

  assert.match(navbar, />运筹</)
  assert.doesNotMatch(navbar, />Stratum</)
  assert.match(
    navbar,
    /className="relative z-10 ml-auto flex items-center gap-3"[\s\S]*<NavigationMenu/
  )
})
