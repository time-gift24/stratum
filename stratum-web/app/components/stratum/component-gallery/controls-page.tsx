import { useTranslation } from "react-i18next"

import {
  ControlLedger,
  SectionCopy,
} from "~/components/stratum/component-gallery-primitives"
import { FeatureParameterCard } from "~/components/stratum/feature-parameter-card"

export function ControlsPage() {
  const { t } = useTranslation()

  return (
    <section
      className="grid min-h-full snap-start place-items-center px-6 py-12 sm:px-10 lg:px-16"
      id="controls"
    >
      <div className="mx-auto grid w-full max-w-7xl gap-10 lg:grid-cols-[minmax(0,0.52fr)_minmax(0,1.48fr)] lg:items-center">
        <SectionCopy
          title={t("componentGallery.controls.title")}
          description={t("componentGallery.controls.description")}
        />
        <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,0.9fr)_minmax(20rem,1.1fr)]">
          <ControlLedger
            actionsLabel={t("componentGallery.controls.actions")}
            inputLabel={t("componentGallery.controls.input")}
            primaryLabel={t("componentGallery.controls.primary")}
            secondaryLabel={t("componentGallery.controls.secondary")}
            outlineLabel={t("componentGallery.controls.outline")}
            disabledLabel={t("componentGallery.controls.disabled")}
            placeholder={t("componentGallery.controls.placeholder")}
          />
          <FeatureParameterCard className="justify-self-center xl:justify-self-end" />
        </div>
      </div>
    </section>
  )
}
