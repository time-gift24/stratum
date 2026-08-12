---
version: 1
slug: "stratum-web-app-site-studio-page-tsx"
primary_target: "stratum-web/app/(site)/studio/page.tsx"
related_targets: ["stratum-web/app/(site)/studio/agents/new/page.tsx","stratum-web/app/(site)/studio/agents/[agent_name]/page.tsx","stratum-web/app/(site)/studio/settings/providers/page.tsx","stratum-web/app/(site)/studio/settings/models/page.tsx"]
---

# Studio management surface

- Scope and mode: `/studio` plus its Agent, Provider, and Model editors; Operate.
- Audience and job: developers and administrators configure real Agent definitions and supported LLM resources against the local management API.
- Primary tasks: find or create an Agent, edit its structured configuration, and reach Provider/Model settings from the far-right settings action.
- Proof and content: only API-backed names, provider/model identity, tool counts, timestamps, schema, revisions, blockers, violations, and one-shot connection-test results.
- Constraints: no Agents tab, explanatory Agents block, prompt excerpts, fake metrics, health lights, monitoring placeholders, resource-config dashboard section, secret reflection, product mock data, or changes to protected reusable components.
- Direction: a quiet Agent-first dashboard with flat information-rich cards and full-page editors. Light is warm-paper soft minimalism inspired by `rbp-portfolio`; dark preserves the existing graphite Stratum world.
- Memorable moment: the restrained settings selector uses one sliding sage underlay while the content and actions remain still and immediately legible.
- Unresolved: none for the first release; future Agent monitoring follows the card grid only when real statistics APIs exist.
