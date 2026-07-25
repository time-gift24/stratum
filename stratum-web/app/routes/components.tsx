/**
 * THESIS: A component calibration bench, not a generic Storybook card wall.
 * OWN-WORLD: Graphite black, white type, precise hairlines, and four restrained signal colors.
 * STORY: Review the system foundations, operate the navigation, then compare control states.
 * FIRST VIEWPORT: A magnetic left rail faces an oversized type specimen and a full-width spectrum.
 * FORM: Brief-pinned precision instrument panel; ranked first for this focused review surface.
 */
"use client"

import {
  Bell,
  Brackets,
  CircleAlert,
  Component,
  Navigation,
  SlidersHorizontal,
} from "lucide-react"
import { useTranslation } from "react-i18next"

import {
  VerticalNavigation,
  type VerticalNavigationItem,
} from "~/components/react-bits/vertical-navigation"
import { Badge } from "~/components/ui/badge"
import { Button } from "~/components/ui/button"
import { Input } from "~/components/ui/input"

export default function ComponentsPage() {
  const { t } = useTranslation()
  const navigationItems: readonly VerticalNavigationItem[] = [
    {
      id: "foundations",
      icon: Brackets,
      label: t("componentGallery.navigation.foundations"),
      href: "#foundations",
      tone: "blue",
    },
    {
      id: "navigation",
      icon: Navigation,
      label: t("componentGallery.navigation.navigation"),
      href: "#navigation",
      tone: "yellow",
    },
    {
      id: "controls",
      icon: SlidersHorizontal,
      label: t("componentGallery.navigation.controls"),
      href: "#controls",
      tone: "magenta",
    },
    {
      id: "states",
      icon: Bell,
      label: t("componentGallery.navigation.states"),
      href: "#states",
      tone: "neutral",
    },
  ]

  return (
    <div className="component-gallery">
      <VerticalNavigation
        items={navigationItems}
        activeId="foundations"
        ariaLabel={t("componentGallery.navigation.label")}
      />

      <main className="component-gallery__main" id="main-content">
        <section className="component-gallery__hero" id="foundations">
          <div className="component-gallery__hero-copy">
            <h1>{t("componentGallery.title")}</h1>
            <p>{t("componentGallery.description")}</p>
          </div>

          <div
            className="component-spectrum"
            role="group"
            aria-label={t("componentGallery.palette.label")}
          >
            <div className="component-spectrum__color" data-color="blue">
              <span>{t("componentGallery.palette.information")}</span>
              <code>#6DB5FF</code>
            </div>
            <div className="component-spectrum__color" data-color="yellow">
              <span>{t("componentGallery.palette.selection")}</span>
              <code>#FEFA3D</code>
            </div>
            <div className="component-spectrum__color" data-color="magenta">
              <span>{t("componentGallery.palette.collaboration")}</span>
              <code>#FF5DE7</code>
            </div>
            <div className="component-spectrum__color" data-color="green">
              <span>{t("componentGallery.palette.action")}</span>
              <code>#78ED9D</code>
            </div>
          </div>
        </section>

        <section className="component-gallery__section" id="navigation">
          <div className="component-gallery__section-copy">
            <h2>{t("componentGallery.verticalNavigation.title")}</h2>
            <p>{t("componentGallery.verticalNavigation.description")}</p>
          </div>

          <div className="navigation-calibration" aria-hidden="true">
            <div className="navigation-calibration__axis" />
            <div className="navigation-calibration__sample" data-size="compact">
              <Component />
            </div>
            <div className="navigation-calibration__sample" data-size="active">
              <Navigation />
            </div>
            <div className="navigation-calibration__sample" data-size="compact">
              <SlidersHorizontal />
            </div>
          </div>

          <dl className="component-gallery__specs">
            <div>
              <dt>{t("componentGallery.verticalNavigation.desktop")}</dt>
              <dd>48–60 px</dd>
            </div>
            <div>
              <dt>{t("componentGallery.verticalNavigation.mobile")}</dt>
              <dd>44 px</dd>
            </div>
            <div>
              <dt>{t("componentGallery.verticalNavigation.motion")}</dt>
              <dd>180 / 16</dd>
            </div>
          </dl>
        </section>

        <section className="component-gallery__section" id="controls">
          <div className="component-gallery__section-copy">
            <h2>{t("componentGallery.controls.title")}</h2>
            <p>{t("componentGallery.controls.description")}</p>
          </div>

          <div className="component-control-ledger">
            <div className="component-control-ledger__row">
              <span>{t("componentGallery.controls.actions")}</span>
              <div className="component-control-ledger__samples">
                <Button>{t("componentGallery.controls.primary")}</Button>
                <Button variant="secondary">
                  {t("componentGallery.controls.secondary")}
                </Button>
                <Button variant="outline">
                  {t("componentGallery.controls.outline")}
                </Button>
                <Button disabled>
                  {t("componentGallery.controls.disabled")}
                </Button>
              </div>
            </div>

            <div className="component-control-ledger__row">
              <label htmlFor="component-gallery-input">
                {t("componentGallery.controls.input")}
              </label>
              <div className="component-control-ledger__samples component-control-ledger__samples--field">
                <Input
                  id="component-gallery-input"
                  placeholder={t("componentGallery.controls.placeholder")}
                />
              </div>
            </div>
          </div>
        </section>

        <section className="component-gallery__section" id="states">
          <div className="component-gallery__section-copy">
            <h2>{t("componentGallery.states.title")}</h2>
            <p>{t("componentGallery.states.description")}</p>
          </div>

          <div className="component-state-line">
            <Badge>{t("componentGallery.states.action")}</Badge>
            <Badge variant="secondary">
              {t("componentGallery.states.neutral")}
            </Badge>
            <Badge className="component-badge--information" variant="outline">
              {t("componentGallery.states.information")}
            </Badge>
            <Badge className="component-badge--selection" variant="outline">
              {t("componentGallery.states.selection")}
            </Badge>
            <Badge className="component-badge--collaboration" variant="outline">
              {t("componentGallery.states.collaboration")}
            </Badge>
            <Badge variant="destructive">
              <CircleAlert data-icon="inline-start" />
              {t("componentGallery.states.error")}
            </Badge>
          </div>
        </section>
      </main>
    </div>
  )
}
