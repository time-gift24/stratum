import { useTranslation } from "react-i18next"

import {
  SectionCopy,
  StateLine,
} from "~/components/stratum/component-gallery-primitives"

export function StatesPage() {
  const { t } = useTranslation()

  return (
    <section
      className="grid min-h-full snap-start place-items-center px-6 py-12 sm:px-10 lg:px-16"
      id="states"
    >
      <div className="mx-auto grid w-full max-w-6xl gap-12 lg:grid-cols-[minmax(0,0.68fr)_minmax(0,1.32fr)] lg:items-center">
        <SectionCopy
          title={t("componentGallery.states.title")}
          description={t("componentGallery.states.description")}
        />
        <StateLine
          action={t("componentGallery.states.action")}
          neutral={t("componentGallery.states.neutral")}
          information={t("componentGallery.states.information")}
          selection={t("componentGallery.states.selection")}
          collaboration={t("componentGallery.states.collaboration")}
          error={t("componentGallery.states.error")}
        />
      </div>
    </section>
  )
}
