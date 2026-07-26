import type { ReactNode } from "react"

type RouteTransitionProps = {
  children: ReactNode
}

export function RouteTransition({ children }: RouteTransitionProps) {
  return <div data-route-page>{children}</div>
}
