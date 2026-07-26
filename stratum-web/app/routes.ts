import { type RouteConfig, index, route } from "@react-router/dev/routes"

export default [
  index("routes/home.tsx"),
  route("chat", "routes/chat.tsx"),
  ...(process.env.NODE_ENV === "production"
    ? []
    : [
        route("component-gallery", "routes/component-gallery-layout.tsx", [
          index("routes/components.tsx"),
        ]),
      ]),
] satisfies RouteConfig
