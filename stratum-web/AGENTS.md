# Stratum Web

## Page and chat layout

- Overview (`/`) and Longzhong (`/longzhong`) are independent routes. Do not place them in a shared scrolling track.
- Navbar tabs navigate between routes with a left/right view transition; they do not scroll to in-page anchors.
- Chat messages use the document scroll. Do not add an internal message scroller.
- Auto-follow is user-controlled: scrolling upward pauses it, content resize must preserve the reading
  position, and it resumes only after the user reaches the actual bottom or activates the scroll-to-bottom
  control.

## Longzhong chat layout constraints (hard)

- The main chat column on `/longzhong` must remain a single centered column. The only horizontal dimension that may be adjusted is the whitespace (gutter / margin) on the left and right sides of this column.
- Do not embed `ChatHistory` into the main layout flow as a permanent left or right rail. It must stay a togglable overlay / drawer.
- `SiteNavbar` and the bottom `PromptInput` are fixed, but their top/bottom offsets from the viewport must be expressed as the outermost `margin` on their fixed containers, not as internal padding or positioned offsets.
- On wide screens (`2xl`+), the history trigger is rendered as a detached pill to the left of the navbar shell; the drawer opens down-left from that trigger with a safe margin from the left edge.
- The Longzhong composer renders adjacent Agent and model dropdowns in its left tool area. A new
  conversation selects the first template by default; switching Agent starts a new uncreated
  conversation and resets the model to that template default. A pre-session model selection is sent
  with the creation request, while an existing-session selection applies to the next message.
- Approval UI may describe only facts carried by the approval event. Do not generate generic reasons,
  effects, risk claims, or reversibility guidance when the backend did not provide them.

## Frontend test policy

- Do not add, restore, or maintain frontend test files under `stratum-web`.

## Component ownership

- Stratum-owned components live in `app/components/stratum/`.
- Keep third-party components in `app/components/react-bits/`, `app/components/ui/`, or `app/components/ai-elements/`.

## Ontology modeling canvas

- The read-only ontology explorer lives at `/ontology`. Its navbar label is “建模” in Chinese and “Modeling” in English; internal domain types and component names continue to use `Ontology`.
- This frontend task must not compose `wyse-ontology-api` into the Rust API host. Read the configured `VITE_WYSE_API_BASE_URL` when available and keep host integration as a separate task.
- Use `@xyflow/react` for the canvas and `@dagrejs/dagre` for a deterministic left-to-right layout. The canvas supports selection, pan, zoom, and fit-to-view; nodes are not draggable and no edit action is exposed.
- Keep responsibilities separated across `OntologyWorkspace`, `OntologySourcePanel`, `OntologyGraphCanvas`, `OntologyInspector`, and a typed `app/lib/ontology-api.ts` client. Example graph data belongs in a separate fixture and must carry an explicit demo marker.
- The desktop layout is a structural three-column workspace: source/type index, graph canvas, and selection inspector. At widths below `1024px`, the canvas occupies the page and the source and inspector panels become accessible drawers with focus return.
- The source panel switches among Tag, Draft, and Revision. Tag accepts a name and defaults to `online`; Draft and Revision use their list endpoints when the API is available. Selecting a source clears stale selection, lays out the graph again, and fits the viewport.
- Load both the graph projection and the selected schema document. Resolve a Tag through `GET /v1/ontology/tags/{name}` and its returned revision, a Draft through `GET /v1/ontology/drafts/{name}`, and a Revision through `GET /v1/ontology/revisions/{id}`. Node details show properties from the schema document; edge details show relation name, endpoints, and cardinality. Selecting an item in the left index must focus and select the matching canvas element.
- When API configuration is absent, the connection fails, or the default `online` schema is missing, show the built-in example graph behind a persistent, explicit “示例模型” status with the reason and a retry action. Do not silently substitute example data after the user explicitly selects a missing or invalid source.
- A valid empty schema uses an instructional empty state. Loading preserves the three-column skeleton and uses skeleton content rather than a centered spinner.
- Keep the visual language flat and structural: 1px separators, no glass, no large soft shadows, no repeated uppercase eyebrow labels, and no nested cards. A dot grid is allowed only inside the actual graph canvas.
- Keyboard users must be able to select nodes and edges through the source index and canvas. Selection cannot rely on color alone; use visible borders/backgrounds and `aria-selected`. Respect reduced motion and the existing contrast and minimum-type-size requirements.
- Do not add frontend test files for this feature. Verify with `pnpm typecheck`, `pnpm build`, and browser checks covering themes, locales, desktop/mobile layouts, keyboard use, reduced motion, API-unconfigured, network-error, empty-schema, invalid-source, normal, and large-graph states.
