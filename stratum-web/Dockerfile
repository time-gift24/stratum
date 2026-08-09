# syntax=docker/dockerfile:1

FROM node:22-alpine AS base
WORKDIR /app
ENV NEXT_TELEMETRY_DISABLED=1
RUN corepack enable \
  && corepack prepare pnpm@10.15.1 --activate

FROM base AS dependencies
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile

FROM dependencies AS builder
COPY . .
RUN pnpm build

FROM base AS runtime
ENV NODE_ENV=production
ENV HOSTNAME=0.0.0.0
COPY --from=builder /app ./
EXPOSE 3000
CMD ["pnpm", "start"]
