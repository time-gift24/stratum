import { useState } from "react"
import { I18nextProvider, useTranslation } from "react-i18next"
import {
  Links,
  Meta,
  Outlet,
  Scripts,
  ScrollRestoration,
  isRouteErrorResponse,
  useLocation,
  useRouteLoaderData,
} from "react-router"

import type { Route } from "./+types/root"
import { GlobalNavigation } from "./components/stratum/global-navigation"
import { ProductShell } from "./components/stratum/product-shell"
import { createI18n } from "./lib/i18n"
import { getRequestLanguage } from "./lib/locale"
import "./app.css"

export function loader({ request }: Route.LoaderArgs) {
  return { language: getRequestLanguage(request) }
}

export function Layout({ children }: { children: React.ReactNode }) {
  const language = useRouteLoaderData<typeof loader>("root")?.language ?? "en"
  const [i18n] = useState(() => createI18n(language))

  return (
    <html lang={language} className="dark" suppressHydrationWarning>
      <head>
        <meta charSet="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <title>Stratum</title>
        <meta
          name="description"
          content="Stratum is an observable Agent OS for running agents, models, tools, and conversations."
        />
        <link rel="icon" type="image/svg+xml" href="/favicon.svg" />
        <Meta />
        <Links />
      </head>
      <body>
        <I18nextProvider i18n={i18n}>
          {children}
          <ScrollRestoration />
          <Scripts />
        </I18nextProvider>
      </body>
    </html>
  )
}

export default function App() {
  const location = useLocation()

  const isComponentGallery =
    location.pathname === "/component-gallery" ||
    location.pathname.startsWith("/component-gallery/")

  if (import.meta.env.DEV && isComponentGallery) {
    return (
      <>
        <GlobalNavigation />
        <Outlet />
      </>
    )
  }

  return (
    <ProductShell>
      <Outlet />
    </ProductShell>
  )
}

export function ErrorBoundary({ error }: Route.ErrorBoundaryProps) {
  const { t } = useTranslation()
  let message = t("errors.unexpectedTitle")
  let details = t("errors.unexpectedDetails")
  let stack: string | undefined

  if (isRouteErrorResponse(error)) {
    message = error.status === 404 ? "404" : t("errors.genericTitle")
    details =
      error.status === 404
        ? t("errors.notFoundDetails")
        : error.statusText || details
  } else if (import.meta.env.DEV && error && error instanceof Error) {
    details = error.message
    stack = error.stack
  }

  return (
    <main className="grid min-h-dvh place-items-center bg-background p-6 text-foreground">
      <div className="w-full max-w-xl rounded-xl border border-border bg-card p-6 shadow-2xl">
        <h1 className="font-heading text-3xl font-medium tracking-tight">
          {message}
        </h1>
        <p className="mt-3 text-muted-foreground">{details}</p>
        {stack && (
          <pre className="mt-5 w-full overflow-x-auto rounded-lg bg-muted p-4 text-xs">
            <code>{stack}</code>
          </pre>
        )}
      </div>
    </main>
  )
}
