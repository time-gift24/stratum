import { useLocale } from "~/components/locale-provider"
import { useWorkspacePager } from "~/components/workspace-pager"

const WORKSPACE_NAVIGATION = [
  { label: "nav.chat", slideIndex: 1 },
  { label: "nav.orchestration", slideIndex: 2 },
] as const

export function SiteNavbar() {
  const { t } = useLocale()
  const { activeSlideIndex, selectSlide } = useWorkspacePager()

  return (
    <header className="site-navbar">
      <div className="site-navbar-shell">
        <a
          href="/"
          className="site-navbar-brand"
          aria-label="运筹 Stratum home"
        >
          <span className="site-navbar-brand-copy">
            <span className="site-navbar-brand-name">运筹</span>
            <span className="site-navbar-brand-product">Stratum</span>
          </span>
        </a>

        <div className="site-navbar-actions">
          {WORKSPACE_NAVIGATION.map((item) => (
            <button
              aria-current={
                activeSlideIndex === item.slideIndex ? "page" : undefined
              }
              className="site-navbar-action"
              key={item.slideIndex}
              onClick={() => selectSlide(item.slideIndex)}
              type="button"
            >
              {t(item.label)}
            </button>
          ))}
        </div>
      </div>
    </header>
  )
}
