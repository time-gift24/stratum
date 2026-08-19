---
version: 1
slug: "stratum-web-app-site-studio-page-tsx"
primary_target: "stratum-web/app/(site)/studio/page.tsx"
related_targets: ["stratum-web/app/(site)/ontologies/page.tsx"]
---

---

version: 1
slug: "stratum-web-app-site-studio-page-tsx"
primary_target: "stratum-web/app/(site)/studio/page.tsx"
related_targets: ["stratum-web/app/(site)/studio/agents/new/page.tsx","stratum-web/app/(site)/studio/agents/[agent_name]/page.tsx","stratum-web/app/(site)/studio/settings/providers/page.tsx","stratum-web/app/(site)/studio/settings/models/page.tsx","stratum-web/app/(site)/ontologies/page.tsx"]
---

# Studio management surface

- Scope and mode: `/studio`（仪表盘）plus its Agent, Provider, and Model editors, and the shared list/form language that `/ontologies` also consumes; Operate.
- Audience and job: developers and administrators configure real Agent definitions and supported LLM resources against the local management API.
- Primary tasks: scan the ResourceCard grid, search, create, edit structured configuration, and reach Provider/Model settings from the top-nav 设置 CTA.
- Proof and content: only API-backed names, provider/model identity, tool counts, timestamps, schema, revisions, blockers, violations, and one-shot connection-test results. StatusChip renders only real API fields such as credential_configured; Agent definitions and Models have no status/default field, so their cards carry no badge by design.
- Constraints: no Agents tab, explanatory blocks, prompt excerpts, fake metrics, health lights, monitoring placeholders, secret reflection, product mock data, hand-rolled base components (CONSTITUTION §15), or changes to protected reusable components.
- Direction: 仪表盘 asset-ledger — 2-column ResourceCard grid (squircle monogram + dashed-separated mono meta rows), flat fieldset forms on shadcn ui/field, ErrorState/NotFoundState dashed flat panels with a path forward (404 is never a dead end). Light warm-paper / dark graphite worlds unchanged.
- Memorable moment: the whole surface reads as one ledger — dashboard, settings, and ontology lists share the same card, and route changes slide along one PAGE_ORDER sequence.
- Unresolved: native select vs Base UI Select was settled on StudioSelect (Base UI); tab triggers and canvas node internals stay hand-rolled by exemption.
