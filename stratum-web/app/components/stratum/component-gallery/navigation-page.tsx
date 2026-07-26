import { useTranslation } from "react-i18next"

import {
  NavigationCalibration,
  SectionCopy,
  SpecificationList,
} from "~/components/stratum/component-gallery-primitives"

export function NavigationPage() {
  const { t } = useTranslation()

  return (
    <section
      className="grid min-h-full snap-start place-items-center px-6 py-12 sm:px-10 lg:px-16"
      id="navigation"
    >
      <div className="mx-auto grid w-full max-w-6xl gap-12 lg:grid-cols-[minmax(0,0.68fr)_minmax(0,1.32fr)] lg:items-center">
        <SectionCopy
          title={t("componentGallery.verticalNavigation.title")}
          description={t("componentGallery.verticalNavigation.description")}
        />
        <div>
          <NavigationCalibration />
          <SpecificationList
            items={[
              {
                label: t("componentGallery.verticalNavigation.desktop"),
                value: "48–60 px",
              },
              {
                label: t("componentGallery.verticalNavigation.mobile"),
                value: "44 px",
              },
              {
                label: t("componentGallery.verticalNavigation.motion"),
                value: "180 / 16",
              },
            ]}
          />
        </div>
      </div>
    </section>
  )
}
