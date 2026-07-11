# React Router + shadcn/ui

This is a template for a new React Router project with React, TypeScript, and shadcn/ui.

## Adding components

To add components to your app, run the following command:

```bash
npx shadcn@latest add button
```

This will place the ui components in the `components` directory.

## Using components

To use the components in your app, import them as follows:

```tsx
import { Button } from "@/components/ui/button";
```

## Agent API development

Copy `.env.example` to `.env.local` and set the API base URL and default
template name. The API origin must appear in `api.allowed_origins` in
`wyse-api` configuration.

```bash
pnpm install
pnpm dev
pnpm typecheck
pnpm test
pnpm build
```
