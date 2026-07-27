import { ControlsPage } from "~/components/stratum/component-gallery/controls-page"
import { FoundationsPage } from "~/components/stratum/component-gallery/foundations-page"
import { NavigationPage } from "~/components/stratum/component-gallery/navigation-page"
import { StatesPage } from "~/components/stratum/component-gallery/states-page"

export default function ComponentsPage() {
  return (
    <main
      className="mt-(--global-nav-offset) h-[calc(100dvh-var(--global-nav-offset))] snap-y snap-mandatory [scrollbar-width:none] overflow-y-auto overscroll-y-contain scroll-smooth motion-reduce:snap-none motion-reduce:scroll-auto"
      id="component-gallery-scroll"
    >
      <FoundationsPage />
      <NavigationPage />
      <ControlsPage />
      <StatesPage />
    </main>
  )
}
