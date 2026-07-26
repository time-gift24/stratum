import type { ComponentType, ReactNode } from "react"

export type BorderGlowProps = {
  children: ReactNode
  className?: string
  edgeSensitivity?: number
  glowColor?: string
  backgroundColor?: string
  borderRadius?: number | string
  glowRadius?: number
  glowIntensity?: number
  coneSpread?: number
  animated?: boolean
  colors?: string[]
  fillOpacity?: number
}

declare const BorderGlow: ComponentType<BorderGlowProps>

export default BorderGlow
