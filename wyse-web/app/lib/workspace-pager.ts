export type PagerMotionPolicy = "animate" | "instant"

export function getPagerMotionPolicy(
  prefersReducedMotion: boolean
): PagerMotionPolicy {
  return prefersReducedMotion ? "instant" : "animate"
}

function clampSlideIndex(index: number, slideCount: number): number {
  const lastSlideIndex = Math.max(0, Math.trunc(slideCount) - 1)

  return Math.min(Math.max(0, Math.trunc(index)), lastSlideIndex)
}

export function resolveSlideIndex(
  currentIndex: number,
  requestedIndex: number,
  slideCount: number,
  isTransitionLocked: boolean
): number {
  if (isTransitionLocked) {
    return clampSlideIndex(currentIndex, slideCount)
  }

  return clampSlideIndex(requestedIndex, slideCount)
}
