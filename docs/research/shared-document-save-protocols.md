# Shared-document save protocols for the Ontology canvas

Research date: 2026-08-08

## Scope and evidence boundary

This note separates mechanisms that are often collapsed into the word
"save": local edit state, the trigger that sends work, durable server commit,
concurrency control, concurrent-operations merge, offline replay, presence,
and recovery when a response is lost. It compares what each mechanism solves
for the current single-tenant Ontology canvas, whose working assumption is a
local canvas copy followed by an atomic whole-document save to PostgreSQL.

Only primary sources are used: HTTP standards, first-party engineering notes,
official framework documentation, and project-owned documentation/source.
Public descriptions of proprietary editors are incomplete, so this note does
not infer undocumented internals. In particular, it does not repeat folklore
about Google Docs; ShareDB is used as the documented OT example instead.

## Findings in brief

- A local working copy and an explicit Save button are user-interaction
  choices, not concurrency or durability protocols. The same conditional
  whole-document request can be sent explicitly or by debounced autosave. A
  durable browser draft protects work that has not reached the server; an
  autosave timer alone does not.
- A strong `ETag` plus required `If-Match` is a standard lost-update guard for
  whole-document replacement. It detects concurrent saves from another tab or
  user but deliberately does not merge them. HTTP defines `412 Precondition
  Failed` for a false condition, while `428 Precondition Required` can require
  callers to send one. [RFC 9110, `If-Match`](https://www.rfc-editor.org/rfc/rfc9110.html#section-13.1.1)
  [RFC 6585, 428](https://www.rfc-editor.org/rfc/rfc6585.html#section-3)
- Operation logs, OT, and CRDTs solve different problems. A log can make
  already-accepted incremental edits durable and replayable without making
  concurrent edits merge correctly. OT transforms type-specific concurrent
  operations. A CRDT gives its own updates convergence properties but still
  needs transport, storage, product conflict semantics, and enforcement of
  application invariants.
- The current whole-document protocol is a credible MVP base. Low-cost ideas
  from shared editors can be borrowed without installing a collaboration
  engine: keep the canvas responsive with an acknowledged/unacknowledged state
  distinction; persist an optional local recovery draft; make the server the
  authority for full-graph validation; retain the rejected candidate on a
  conflict; and reconcile an uncertain commit by reading the canonical
  resource.
- WebSockets, presence, incremental operation endpoints, an application
  journal, OT, CRDT storage, durable tombstones, and automatic offline merge
  are premature unless live multi-user or offline-first editing enters the
  destination. They are a second consistency model, not a transparent
  optimization of conditional `PUT`.
- Stable client-generated child IDs and field-level domain boundaries preserve
  options. Reassigning IDs on save, addressing edits by array indexes, treating
  the entire graph as one last-writer-wins register, or overloading the current
  snapshot revision as an operation/actor/causal identity would make a later
  collaboration migration materially harder.
- There is an HTTP constraint in the current API direction: RFC 9110 forbids a
  validator such as `ETag` in a successful `PUT` response if the server
  transformed the request representation before saving it. Incrementing a
  `revision` field inside the submitted JSON, injecting defaults, or returning
  reordered/normalized content can be such a transformation. The API must
  either make the write representation itself the saved representation, omit
  the validator and re-`GET`, or use processing semantics other than strict
  replacement. [RFC 9110, PUT](https://www.rfc-editor.org/rfc/rfc9110.html#section-9.3.4)

## MVP disposition matrix

| Mechanism | MVP disposition | What it buys now | Boundary / later trigger |
| --- | --- | --- | --- |
| Local acknowledged/candidate/in-flight states | Borrow now | Responsive canvas and no accidental loss of newer local edits | Client state only; not durable by itself |
| Explicit save using strong ETag/CAS | Borrow now | Atomic whole-graph save and stale-writer detection | Detects conflicts; does not merge them |
| Browser-local recovery draft | Small optional addition now | Survives refresh/crash before server acknowledgement | Store base tag with candidate; do not call it automatic offline merge |
| Server validation before canonical commit | Borrow now | Preserves graph invariants for every caller | Client validation remains advisory |
| Uncertain-commit `GET` reconciliation | Borrow now | Determines whether a lost response hid a successful commit | Add durable attempt identity only if transparent retries become required |
| Autosave | Defer as a product trigger, reuse same protocol | Shorter normal unsent window | Adds write load and conflict frequency; not a new consistency model |
| Server operation journal + checkpoints | Defer | Durable/replayable incremental accepted edits | Needed when edits are acknowledged before full snapshot commit, or history/fanout is required |
| OT or CRDT operation model | Defer | Automatic concurrent/offline merge within designed semantics | Triggered by live multi-user or offline-first requirements; requires a new domain protocol |
| WebSocket subscription and presence | Defer | Live remote changes, cursors, selections, online state | Presence must stay outside durable Ontology/canvas state |

Before freezing OpenAPI, the `PUT` row has one mandatory sub-decision: either
the submitted representation is exactly the saved representation so the
successful response may carry the new `ETag`, or normalization/server-managed
body fields require a re-`GET` or different method semantics.

## The mechanisms are separate layers

### 1. Local edit state

Shared editors normally apply a user gesture to a local model immediately and
distinguish it from acknowledgement by the authoritative service. Figma's
first-party description says property changes are applied immediately instead
of waiting for the server; while a local change is unacknowledged, the client
keeps it as its best prediction instead of temporarily overwriting it with an
older acknowledged value. [Figma: How Figma's multiplayer technology works](https://www.figma.com/blog/how-figmas-multiplayer-technology-works/)

This is latency hiding, not persistence. For the Ontology canvas it maps to at
least three logical copies or states, even if they share data structurally:

- the last acknowledged server document and validator;
- the current local candidate, including unsaved edits;
- an in-flight candidate whose acknowledgement is pending.

Without that distinction, a response or background refresh can erase newer
local gestures. With it, explicit save, autosave, validation failure, and
conflict can all preserve the user's candidate. This is an inference from the
acknowledged/unacknowledged patterns above, not a claim about one required UI.

### 2. Explicit save versus autosave

An explicit Save button chooses when to send a mutation. Debounced autosave
chooses a different trigger. Neither choice defines atomicity, merge behavior,
or crash recovery.

Figma's documented design continuously exchanges incremental changes over a
WebSocket after the initial document download. ShareDB's client applies an
operation locally, sends it to the server, and calls the completion callback
when the server has committed it. These are operation-sync systems, not whole
snapshot endpoints with a hidden Save button. [Figma: multiplayer setup](https://www.figma.com/blog/how-figmas-multiplayer-technology-works/)
[ShareDB `Doc.submitOp`](https://share.github.io/sharedb/api/doc#submitop)

For a snapshot API, explicit save and autosave can therefore share exactly the
same `If-Match` contract. Autosave reduces the normal unsent interval but
creates more writes and more opportunities for two tabs to race. Browser-local
persistence is the separate mechanism that survives a tab or process crash
before server acknowledgement.

### 3. Local durable drafts and offline queues

Yjs demonstrates browser-local durability by persisting document updates to
IndexedDB; on a later visit it loads the local document and only the latest
updates need to synchronize over the network. Its providers can combine local
database and network synchronization. [Yjs: Offline Support](https://docs.yjs.dev/getting-started/allowing-offline-editing)

Automerge likewise separates the CRDT from repository plumbing. A repository
can attach local storage and network adapters; a `DocHandle` change is stored
locally and transmitted to peers. Its IndexedDB adapter is safe for concurrent
use from multiple tabs, but the documentation is explicit that tabs do not
live-update one another unless a channel/network adapter such as
`BroadcastChannel` is added. [Automerge: Concepts](https://automerge.org/docs/reference/concepts/)
[Automerge: Storage](https://automerge.org/docs/reference/repositories/storage/)

Figma documents a different reconnect pattern: after offline editing, the
client downloads a fresh document, reapplies offline edits over that state,
then resumes live synchronization. Figma also depends on client-generated
globally unique object IDs because creation must work offline. [Figma:
multiplayer reconnect and object creation](https://www.figma.com/blog/how-figmas-multiplayer-technology-works/)

These examples show two independent requirements:

1. a durable local record so a browser restart does not lose unsent work;
2. a merge/rebase protocol for applying that work after the remote base has
   changed.

A saved IndexedDB snapshot containing `{ontology_id, base_etag, candidate}` can
solve the first requirement for this MVP. It does not solve the second. After
reconnect, an unchanged `ETag` permits the ordinary save; a changed `ETag`
still needs deliberate conflict UX or a future operation merge model.

### 4. Whole snapshot plus ETag/CAS

HTTP entity tags are opaque validators. `If-Match` uses strong comparison and
is specifically intended to prevent one agent from overwriting another's
parallel update. If the condition is false, the server must not perform the
method and may return `412`. [RFC 9110, entity tags](https://www.rfc-editor.org/rfc/rfc9110.html#section-8.8.3)
[RFC 9110, `If-Match`](https://www.rfc-editor.org/rfc/rfc9110.html#section-13.1.1)

This is also a deployed document/file API pattern: Microsoft Graph accepts
`If-Match` when updating a `driveItem` and returns `412` when the supplied tag
does not match. [Microsoft Graph: Update DriveItem properties](https://learn.microsoft.com/en-us/graph/api/driveitem-update?view=graph-rest-1.0)

The guarantee is intentionally narrow:

- it prevents a lost update at the whole-resource boundary;
- it serializes successful replacements through one current validator;
- it says nothing about how two candidates should merge;
- it does not preserve an operation history;
- it does not protect unsent browser state;
- it does not notify other open clients that a change occurred.

Thus it is sufficient for an explicit-save MVP and for detecting multi-tab
concurrency. A `412` is not itself a satisfactory editor experience: the
client should retain the rejected candidate and can fetch the new server state
for comparison. Automatic three-way merge additionally requires retaining the
base document, not only its tag.

#### Strong ETag and normalized PUT responses

RFC 9110 defines `PUT` as replacing the target resource state with the state in
the request representation. A successful `PUT` response must not include a
validator unless the request representation was saved without transformation
and that validator reflects the new representation. This lets the authoring
client know that the copy it retained is the new resource without another
read. [RFC 9110, PUT](https://www.rfc-editor.org/rfc/rfc9110.html#section-9.3.4)

This has concrete consequences for the proposed Ontology contract:

- Mapping the same representation into normalized PostgreSQL rows is an
  internal storage choice and does not by itself change HTTP resource state.
- Reordering semantically ordered JSON arrays, filling omitted defaults,
  dropping submitted fields, or changing a submitted `revision` value means
  the returned/current representation can differ from the submitted one.
- If server-owned revision metadata remains inside the JSON body and advances
  on save, the request body is not literally the post-save representation
  unless the protocol makes the next revision part of the submitted desired
  state.

Candidate resolutions are to keep the validator only in HTTP metadata and make
the submitted document the exact saved representation; to accept a processing
operation and use `POST`/`PATCH` semantics; or to omit `ETag` on the transformed
`PUT` response and immediately read the canonical representation. Selecting
among them is an API decision, not a storage decision.

### 5. Incremental operation logs and snapshots

An operation log records changes instead of only the latest materialized
document. It can serve durability, history, incremental distribution, or all
three, but none of those uses automatically gives operations merge semantics.

Figma's multiplayer service is authoritative for validation, ordering, and
conflict resolution. Its earlier durability design kept current document state
in memory and periodically wrote full checkpoints. Figma added a durable
journal where each accepted change receives a per-file sequence number and
each checkpoint records its sequence; recovery loads the checkpoint and
replays newer journal entries. [Figma: Making multiplayer more reliable](https://www.figma.com/blog/making-multiplayer-more-reliable/)

Fluid Framework also combines ordered operations with snapshots/summaries. A
summary represents container state at a particular sequence number, is
recorded through the ordering service, and lets future clients load a recent
snapshot before processing later operations. [Fluid Framework:
Summarization](https://fluidframework.com/docs/concepts/summarizer)

ShareDB commits both an operation and its updated snapshot, stores submitted
operations by default, and can create milestone snapshots so historical state
does not require replay from document creation. [ShareDB: Op submission](https://share.github.io/sharedb/middleware/op-submission)
[ShareDB: Document history](https://share.github.io/sharedb/document-history)

The current Ontology server is different from Figma's former volatile
checkpoint layer: a successful whole-document request is intended to commit
the complete graph transactionally to PostgreSQL before acknowledgement. An
additional application journal would not improve merge behavior and is not
needed merely to reconstruct an already-committed current snapshot. It becomes
useful if the service later acknowledges incremental edits before a full
snapshot is materialized, needs history, or needs efficient incremental fanout.

### 6. Operational Transformation

OT defines operations for a document type and transforms concurrent operations
so they can be applied against a common evolving state. It is not a generic
"send JSON patches" switch: correctness depends on the operation type and its
transformation rules.

ShareDB is a documented JSON OT implementation. The client document has a
server version; a local operation is applied immediately, sent to the server,
and acknowledged after commit. The server's lifecycle exposes the old snapshot
before apply, the new snapshot before commit, and an `afterWrite` point where
the operation and snapshot are canonical. [ShareDB: `Doc`](https://share.github.io/sharedb/api/doc)
[ShareDB: Op submission](https://share.github.io/sharedb/middleware/op-submission)

For an Ontology graph, an OT route would first need a complete operation
algebra: create/delete/rename an entity, change a typed property, move durable
canvas coordinates, add/remove an endpoint, and define every concurrent pair.
It would also need transformation or rejection rules that preserve unique
names, ownership, endpoint existence, and deletion semantics. The current
snapshot DTO does not supply those semantics, so adopting OT now would be a
new domain protocol rather than an implementation detail.

### 7. CRDT documents

Yjs encodes incremental document updates that are commutative, associative,
and idempotent: replicas converge after receiving all updates even if updates
arrive in different orders or more than once. State vectors identify what a
peer already knows so only missing differences need be sent. [Yjs: Document
Updates](https://docs.yjs.dev/api/document-updates)

Automerge models a document as a JSON-like object with commit history and merge
rules. Independently changed documents can be merged. Concurrent writes to the
same property still have product semantics: Automerge selects a deterministic
winner but retains all concurrent values in a conflict object that an
application may expose and later resolve. [Automerge: Concepts](https://automerge.org/docs/reference/concepts/)
[Automerge: Conflicts](https://automerge.org/docs/reference/documents/conflicts/)

CRDT convergence is not the same as Ontology validity. Two convergent updates
can collectively create a duplicate machine name, a dangling Link Type, or a
forbidden ownership change. A server-authoritative product still needs to
validate the merged candidate and define what happens to rejected updates.
Figma illustrates this separation in a custom, CRDT-inspired centralized
system: its server orders last-writer-wins property changes and rejects parent
updates that would form a cycle; clients treat the server as ultimate
authority. [Figma: property and tree synchronization](https://www.figma.com/blog/how-figmas-multiplayer-technology-works/)

Introducing Yjs or Automerge would also change persistence and API payloads to
carry CRDT updates, causal/state metadata, deleted-object history or garbage
collection policy, actor identities, and sync state. Wrapping today's entire
Ontology JSON in one CRDT register would converge but reduce concurrent edits
to whole-document last-writer-wins behavior, which gains little over CAS.

### 8. Server-authoritative validation

Validation must happen against the candidate that would become canonical, not
only against each isolated client gesture. Figma's central server rejects
updates that would create an invalid cycle. ShareDB's documented lifecycle
allows checks against the updated snapshot at the commit hook and identifies
`afterWrite` as the earliest point where the operation and snapshot are known
to be canonical. [Figma: tree synchronization](https://www.figma.com/blog/how-figmas-multiplayer-technology-works/)
[ShareDB: Op submission](https://share.github.io/sharedb/middleware/op-submission)

For the current snapshot protocol this maps cleanly to: validate the complete
candidate and its `If-Match` precondition, make zero writes for any violation,
commit the complete replacement atomically, then acknowledge. Client-side
validation remains useful for immediate feedback but cannot be authoritative
because another client or a non-browser caller can race or bypass it.

If incremental collaboration arrives, rejection becomes harder: the client may
have optimistically applied later operations on top of a now-rejected one. The
protocol then needs rollback/rebase semantics. ShareDB documents that error
recovery can perform a hard rollback and refetch the snapshot. [ShareDB: `Doc`
load event](https://share.github.io/sharedb/api/doc#load)

### 9. Presence is not document state

Presence answers who is connected, where a cursor is, or what a peer is
selecting. It should not be confused with durable canvas coordinates or the
Ontology schema.

Yjs keeps awareness outside the Yjs document because it does not need to
persist across sessions; a disconnected client's awareness eventually
disappears. [Yjs: Awareness & Presence](https://docs.yjs.dev/getting-started/adding-awareness)

For this canvas, node coordinates are durable document/editor state because
they must survive reload. Remote cursors, current selection, viewport, and
online status are ephemeral presence if real-time collaboration is added. A
separate presence channel can therefore be added later without changing the
Ontology save document.

### 10. Recovery after an uncertain commit

A transport failure can occur after the database committed but before the
client received the response. HTTP defines `PUT` as idempotent and permits an
identical request to be retried after a connection failure. With `If-Match`, a
retry using the old tag will normally find a false precondition; RFC 9110 also
allows the server to return success if it can determine that the requested
change was already applied, while warning that this inference can be risky for
similar concurrent requests. [RFC 9110, idempotent methods](https://www.rfc-editor.org/rfc/rfc9110.html#section-9.2.2)
[RFC 9110, `If-Match`](https://www.rfc-editor.org/rfc/rfc9110.html#section-13.1.1)

This creates three distinguishable strategies:

1. **Read and compare.** Mark the save as uncertain, `GET` the canonical
   document and new tag, and compare its client-controlled state with the
   in-flight candidate. Equal means the desired state is present; different
   means preserve the candidate and enter conflict/rebase UX.
2. **Recognize a repeated attempt.** Give each save attempt a client-generated
   identity and retain enough durable server result data to return the original
   result on retry. This removes equality ambiguity but adds an API field,
   retention policy, and durable deduplication state.
3. **Return success for an already-equal current state.** This uses RFC 9110's
   allowance but cannot prove which writer produced that state without an
   attempt identity. It also must not increment the revision again.

The minimal first strategy needs no operation log. A blind retry that increments
the revision twice would violate the intended effect of idempotent replacement;
an unconditional overwrite would defeat `If-Match`.

## Capability comparison

| Mechanism | Single-user browser crash safety | Multi-tab concurrent edits | Multi-user real time | Offline editing and later sync |
| --- | --- | --- | --- | --- |
| In-memory local candidate | No | No coordination | No | Only while the tab remains alive |
| IndexedDB snapshot/draft | Yes for unsent local state | Detect/coordinate only if paired with tag checks or a tab channel | No | Preserves work, but does not merge a changed server base |
| Whole snapshot + strong ETag/CAS | Server state is durable after commit; unsent work is not | Detects stale save and prevents overwrite | No live delivery or merge | Can submit later only if base tag still matches; otherwise conflict |
| Server operation log + snapshots | Protects acknowledged incremental server edits and enables replay | Orders/records operations but needs merge semantics | Distribution-ready when paired with a live channel | Needs a durable client queue and rebase/merge rules |
| OT | Not persistence by itself | Transforms supported concurrent operations | Yes when paired with server/version/transport | Possible, but queued ops must be transformed over intervening history |
| CRDT updates | Not persistence by itself; local adapter can provide it | Deterministically merges supported updates | Yes when paired with provider/transport | Designed to merge independently produced updates; application invariants remain |
| Presence/awareness | Intentionally no | Shows peers, does not protect content | Yes for ephemeral peer state | Offline presence normally disappears |

The table separates guarantees: no one row is a complete product. Yjs, for
example, combines CRDT updates with separate IndexedDB and network providers;
Automerge similarly composes its CRDT/sync format with repository storage and
network adapters. [Yjs: Document Updates](https://docs.yjs.dev/api/document-updates)
[Yjs: Offline Support](https://docs.yjs.dev/getting-started/allowing-offline-editing)
[Automerge: Concepts](https://automerge.org/docs/reference/concepts/)

## What can be borrowed for the MVP

These are low-cost candidates compatible with the existing destination; they
do not constitute the final product decision.

1. **Make acknowledgement visible in the client state machine.** Track clean,
   dirty, saving, saved, invalid, stale/conflict, offline, and
   uncertain-result states. Keep edits responsive and never replace a newer
   candidate with an older response.
2. **Use a required strong validator for every destructive replacement.** An
   opaque `ETag`/`If-Match` contract provides standard multi-tab lost-update
   protection. Use `428` when a mutating caller omits the required condition
   and `412` when it is false.
3. **Keep full validation and commit server-authoritative.** A success means
   the whole graph is canonical in PostgreSQL; a validation or condition
   failure means zero writes.
4. **Preserve the local candidate on all failures.** For a `412`, fetch the
   current canonical graph and offer a comparison/reload/reapply path instead
   of silently discarding either side.
5. **Consider a browser-local recovery draft.** Persist the candidate and its
   base tag after local changes, clear it only after known server
   acknowledgement, and make recovery status explicit. This supplies crash
   safety without claiming automatic offline merge.
6. **Define uncertain-commit reconciliation.** At minimum re-read and compare;
   consider a durable attempt identity only if retries must be transparent.
7. **Keep durable canvas layout separate from ephemeral presence.** Existing
   node coordinates stay in the saved document; remote cursor/selection state
   can use a future transient channel.
8. **Resolve the strict-PUT representation issue before freezing OpenAPI.** A
   transformed request and an `ETag` in the same successful `PUT` response are
   constrained by RFC 9110.

## What is premature for the MVP

- A WebSocket document session or polling subscription: there is no current
  live multi-user destination.
- An incremental operation API or durable application operation journal: the
  current atomic PostgreSQL snapshot is already the acknowledged durability
  boundary.
- OT: the Ontology-specific concurrent operation algebra and every transform
  pair have not been designed.
- CRDT persistence: it adds actor/causal metadata, binary update/storage
  formats, garbage collection, conflict UX, and a new validation/rejection
  problem.
- Automatic offline rebase/merge: a durable local draft can be added without
  promising it.
- Presence, remote cursors, user colors, and edit indicators: these are useful
  only with a live shared session.
- Historical versions, tombstones, and checkpoint compaction: useful for an
  operation/history model, but outside the current hard-delete current-state
  destination.

## Choices that preserve or foreclose later collaboration

### Preserve now

- **Immutable IDs for every addressable entity.** Figma's object model uses
  object IDs and requires client-generated uniqueness for offline creation.
  The chosen client-generated UUIDv7 IDs provide the same addressability
  property without copying Figma's protocol. [Figma: object creation](https://www.figma.com/blog/how-figmas-multiplayer-technology-works/)
- **Treat collections as ID-keyed domain sets, not index-addressed meaning.**
  JSON arrays can remain the snapshot encoding, but a future operation should
  name `ObjectTypeId`, `PropertyId`, or `LinkTypeId`, not “array element 7.”
- **Keep merge granularity visible.** The domain already separates Object
  Types, their Properties, Link Types, and per-object canvas positions. That
  leaves room for future field/entity-level operations; collapsing them into
  one opaque binary register would not.
- **Keep snapshot revision opaque and aggregate-scoped.** It can remain a CAS
  generation even if a later collaboration engine adds independent operation
  IDs, actor IDs, sequence numbers, or causal state.
- **Keep server validation deterministic and independent of transport.** The
  same candidate validator can guard a snapshot today and a materialized
  operation result later.
- **Allow an additional protocol beside the snapshot resource.** The existing
  `GET`/replacement resource can stay useful for initial load, export, repair,
  and resynchronization if a later `/operations` or WebSocket sync protocol is
  added.

### Foreclose or make migration expensive

- Reassigning child IDs on each save or using mutable names as references.
- Making array order the only identity of nodes, properties, links, or canvas
  positions.
- Treating every graph change as whole-document last-writer-wins and later
  expecting field-level conflict recovery without a retained base or history.
- Encoding operation identity, actor identity, or causal order into the current
  integer revision. Those concepts have different cardinality and lifetime.
- Mixing presence into durable Ontology/canvas content, which would cause
  transient cursor traffic to create revisions and persistence churn.
- Claiming today's hard deletion is already offline-safe. A later operation
  model must define deletion versus concurrent update and may need tombstones
  or retained operations; current-state rows alone cannot answer that history.
- Exposing backend row layout as the collaboration protocol. Normalized rows
  are a persistence projection; future operations should express domain
  intent and invariants, not SQL mutations.

Whole-document `GET` and conditional replacement do not themselves foreclose
real-time collaboration. They do mean that later collaboration is an explicit
protocol and persistence migration, not a flag that can be enabled without new
semantic decisions.

## Primary sources

- [RFC 9110: HTTP Semantics](https://www.rfc-editor.org/rfc/rfc9110.html)
- [RFC 6585: Additional HTTP Status Codes](https://www.rfc-editor.org/rfc/rfc6585.html)
- [Microsoft Graph: Update DriveItem properties](https://learn.microsoft.com/en-us/graph/api/driveitem-update?view=graph-rest-1.0)
- [Figma: How Figma's multiplayer technology works](https://www.figma.com/blog/how-figmas-multiplayer-technology-works/)
- [Figma: Making multiplayer more reliable](https://www.figma.com/blog/making-multiplayer-more-reliable/)
- [ShareDB: `Doc` API](https://share.github.io/sharedb/api/doc)
- [ShareDB: Op submission](https://share.github.io/sharedb/middleware/op-submission)
- [ShareDB: Document history](https://share.github.io/sharedb/document-history)
- [Yjs: Document Updates](https://docs.yjs.dev/api/document-updates)
- [Yjs: Offline Support](https://docs.yjs.dev/getting-started/allowing-offline-editing)
- [Yjs: Awareness & Presence](https://docs.yjs.dev/getting-started/adding-awareness)
- [Automerge: Concepts](https://automerge.org/docs/reference/concepts/)
- [Automerge: Conflicts](https://automerge.org/docs/reference/documents/conflicts/)
- [Automerge: Repository Storage](https://automerge.org/docs/reference/repositories/storage/)
- [Fluid Framework: Distributed data structures](https://learn.microsoft.com/en-us/azure/azure-fluid-relay/concepts/data-structures)
- [Fluid Framework: Summarization](https://fluidframework.com/docs/concepts/summarizer)
