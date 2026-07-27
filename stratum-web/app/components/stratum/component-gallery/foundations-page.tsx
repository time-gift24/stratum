import { useTranslation } from "react-i18next"

import {
  PaletteSpectrum,
  type SpectrumItem,
} from "~/components/stratum/component-gallery-primitives"

export function FoundationsPage() {
  const { t } = useTranslation()
  const spectrumItems: readonly SpectrumItem[] = [
    {
      label: t("componentGallery.palette.information"),
      value: "#6DB5FF",
      className: "bg-chart-1",
    },
    {
      label: t("componentGallery.palette.selection"),
      value: "#FEFA3D",
      className: "bg-chart-2",
    },
    {
      label: t("componentGallery.palette.collaboration"),
      value: "#FF5DE7",
      className: "bg-chart-3",
    },
    {
      label: t("componentGallery.palette.action"),
      value: "#78ED9D",
      className: "bg-chart-4",
    },
  ]

  return (
    <section
      className="grid min-h-full snap-start place-items-center px-6 py-12 sm:px-10 lg:px-16"
      id="foundations"
    >
      <div className="mx-auto w-full max-w-6xl">
        <div className="mb-12 max-w-4xl">
          <h1 className="font-heading text-5xl leading-none font-medium tracking-[-0.035em] sm:text-7xl lg:text-8xl">
            {t("componentGallery.title")}
          </h1>
          <p className="mt-6 max-w-2xl text-base leading-7 text-muted-foreground sm:text-lg">
            {t("componentGallery.description")}
          </p>
        </div>
        <PaletteSpectrum
          label={t("componentGallery.palette.label")}
          items={spectrumItems}
        />
      </div>
    </section>
  )
}
