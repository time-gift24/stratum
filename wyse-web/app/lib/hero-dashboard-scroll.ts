export function hasInitialUserIntent(
  locationHash: string,
  scrollY: number
): boolean {
  return locationHash.length > 0 || scrollY !== 0
}

export function shouldAutoScroll(
  hasUserIntent: boolean,
  prefersReducedMotion: boolean
): boolean {
  return !hasUserIntent && !prefersReducedMotion
}
