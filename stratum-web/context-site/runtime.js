;(() => {
  "use strict"

  const reduceMotion = window.matchMedia(
    "(prefers-reduced-motion: reduce)"
  ).matches
  const gsap = window.gsap
  const tabs = Array.from(document.querySelectorAll("[data-surface-tab]"))
  const surfaces = Array.from(document.querySelectorAll("[data-surface]"))
  const dockPanels = Array.from(document.querySelectorAll("[data-dock-panel]"))
  const scrollPositions = new Map()
  let activeSurface = "context"
  let sectionObserver

  const parseHash = () => {
    const [surface, target] = window.location.hash.replace(/^#/, "").split("/")
    return {
      surface: surface === "todo" ? "todo" : "context",
      target: target || undefined,
    }
  }

  const sectionId = (surface, target) => `${surface}-${target}`

  const updateRoute = (hash, replace) => {
    const url = new URL(window.location.href)
    url.hash = hash
    if (replace) history.replaceState(null, "", url.href)
    else history.pushState(null, "", url.href)
  }

  const updateTabs = (surface) => {
    for (const tab of tabs) {
      const selected = tab.dataset.surfaceTab === surface
      tab.setAttribute("aria-selected", String(selected))
      tab.tabIndex = selected ? 0 : -1
    }
    for (const panel of surfaces)
      panel.hidden = panel.dataset.surface !== surface
    for (const panel of dockPanels)
      panel.hidden = panel.dataset.dockPanel !== surface
  }

  const updateCurrentDock = (target) => {
    for (const link of document.querySelectorAll("[data-nav-target]")) {
      if (
        link.closest("[data-dock-panel]")?.dataset.dockPanel !== activeSurface
      )
        continue
      if (link.dataset.navTarget === target)
        link.setAttribute("aria-current", "location")
      else link.removeAttribute("aria-current")
    }
  }

  const syncReadingHash = (target) => {
    const route = parseHash()
    const currentTarget =
      document.getElementById(sectionId(activeSurface, route.target)) ??
      document.getElementById(`concept-${route.target}`)
    const currentSection = currentTarget?.closest("[data-nav-section]")
    if (currentSection?.dataset.navSection === target) return
    updateRoute(`#${activeSurface}/${target}`, true)
  }

  const observeSections = () => {
    sectionObserver?.disconnect()
    const sections = Array.from(
      document.querySelectorAll(
        `[data-surface="${activeSurface}"] [data-nav-section]`
      )
    )
    sectionObserver = new IntersectionObserver(
      (entries) => {
        const visible = entries
          .filter((entry) => entry.isIntersecting)
          .sort(
            (a, b) =>
              Math.abs(a.boundingClientRect.top) -
              Math.abs(b.boundingClientRect.top)
          )[0]
        if (visible) {
          const target = visible.target.dataset.navSection
          updateCurrentDock(target)
          syncReadingHash(target)
        }
      },
      { rootMargin: "-20% 0px -65% 0px", threshold: [0, 0.08] }
    )
    for (const section of sections) sectionObserver.observe(section)
  }

  const switchSurface = (surface, target, options = {}) => {
    const { restore = true, replaceHash = false } = options
    if (surface !== activeSurface) {
      scrollPositions.set(activeSurface, window.scrollY)
      activeSurface = surface
      updateTabs(surface)
      observeSections()
      if (!reduceMotion && gsap) {
        const panel = document.querySelector(`[data-surface="${surface}"]`)
        gsap.fromTo(
          panel,
          { autoAlpha: 0.45, y: 12 },
          {
            autoAlpha: 1,
            y: 0,
            duration: 0.36,
            ease: "power3.out",
            clearProps: "opacity,visibility,transform",
          }
        )
      }
    }

    if (target) {
      const element =
        document.getElementById(sectionId(surface, target)) ??
        document.getElementById(`concept-${target}`)
      if (element)
        element.scrollIntoView({
          behavior: reduceMotion ? "auto" : "smooth",
          block: "start",
        })
    } else if (restore) {
      window.scrollTo({
        top: scrollPositions.get(surface) ?? 0,
        behavior: reduceMotion ? "auto" : "smooth",
      })
    }

    const nextHash = `#${surface}/${target || "overview"}`
    if (replaceHash) updateRoute(nextHash, true)
    else if (window.location.hash !== nextHash) updateRoute(nextHash, false)
  }

  for (const tab of tabs) {
    tab.addEventListener("click", () => {
      const surface = tab.dataset.surfaceTab
      if (surface === activeSurface) return
      switchSurface(surface, undefined)
    })
    tab.addEventListener("keydown", (event) => {
      if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key))
        return
      event.preventDefault()
      const next =
        event.key === "Home"
          ? "context"
          : event.key === "End"
            ? "todo"
            : activeSurface === "context"
              ? "todo"
              : "context"
      if (next !== activeSurface) switchSurface(next, undefined)
      document.querySelector(`[data-surface-tab="${next}"]`)?.focus()
    })
  }

  for (const link of document.querySelectorAll(
    "a[href^='#context/'], a[href^='#todo/']"
  )) {
    link.addEventListener("click", (event) => {
      if (
        event.defaultPrevented ||
        event.button !== 0 ||
        event.metaKey ||
        event.ctrlKey ||
        event.shiftKey ||
        event.altKey
      )
        return
      const hash = new URL(link.href).hash
      const [surface, target] = hash.replace(/^#/, "").split("/")
      if (!target) return
      event.preventDefault()
      switchSurface(surface, target)
    })
  }

  const depthButton = document.querySelector("[data-open-depth]")
  const evidenceDetails = Array.from(
    document.querySelectorAll('[data-surface="context"] details.evidence')
  )
  const syncDepthButton = () => {
    if (!depthButton) return
    const allOpen =
      evidenceDetails.length > 0 && evidenceDetails.every((item) => item.open)
    depthButton.setAttribute("aria-expanded", String(allOpen))
    depthButton.textContent = allOpen ? "收起全部证据" : "展开全部证据"
    const url = new URL(window.location.href)
    if (allOpen) url.searchParams.set("evidence", "all")
    else url.searchParams.delete("evidence")
    history.replaceState(null, "", url.href)
  }
  for (const item of evidenceDetails)
    item.addEventListener("toggle", syncDepthButton)
  depthButton?.addEventListener("click", () => {
    const details = evidenceDetails
    const shouldOpen = details.some((item) => !item.open)
    for (const item of details) item.open = shouldOpen
    syncDepthButton()
  })
  if (new URL(window.location.href).searchParams.get("evidence") === "all") {
    for (const item of evidenceDetails) item.open = true
  }
  syncDepthButton()

  const dock = document.querySelector(".side-dock")
  if (
    dock &&
    window.matchMedia("(min-width: 901px)").matches &&
    !reduceMotion
  ) {
    const animateLinks = (activeLink) => {
      const links = Array.from(
        dock.querySelectorAll("[data-dock-panel]:not([hidden]) a")
      )
      const activeIndex = links.indexOf(activeLink)
      links.forEach((link, index) => {
        const distance =
          activeIndex < 0 ? Infinity : Math.abs(index - activeIndex)
        const target =
          activeIndex < 0
            ? 1
            : 1 + 0.18 * Math.exp(-(distance * distance) / 1.5)
        gsap.to(link, {
          scale: target,
          duration: 0.24,
          ease: "power2.out",
          overwrite: true,
        })
      })
    }

    dock.addEventListener("pointerover", (event) => {
      const link = event.target.closest("a")
      if (link) animateLinks(link)
    })
    dock.addEventListener("pointerleave", () => animateLinks(null))
  }

  window.addEventListener("hashchange", () => {
    const route = parseHash()
    switchSurface(route.surface, route.target, { replaceHash: true })
  })

  const route = parseHash()
  activeSurface = route.surface
  updateTabs(route.surface)
  observeSections()
  if (!reduceMotion && gsap) {
    gsap.fromTo(
      ".hero-copy > *",
      { autoAlpha: 0, y: 24 },
      {
        autoAlpha: 1,
        y: 0,
        duration: 0.72,
        stagger: 0.08,
        ease: "power3.out",
        clearProps: "opacity,visibility,transform",
      }
    )
    gsap.fromTo(
      ".hero-station",
      { autoAlpha: 0, x: 18 },
      {
        autoAlpha: 1,
        x: 0,
        duration: 0.5,
        stagger: 0.07,
        delay: 0.18,
        ease: "power3.out",
        clearProps: "opacity,visibility,transform",
      }
    )
  }
  requestAnimationFrame(() =>
    switchSurface(route.surface, route.target, { replaceHash: true })
  )
})()
