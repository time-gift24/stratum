# Ontology schema semantics for the MVP

## Decision

The MVP should be a closed-world schema editor, not a reasoning system and not a
compatibility layer. Its aggregate consists only of an Ontology, Object Types,
their owned Properties, and Link Types. A save submits the complete desired
schema, validates the complete candidate, and replaces the persisted aggregate
atomically when the supplied revision still matches.

This document borrows semantic patterns from standards and official platform
documentation. It does not copy their identifiers, wire formats, or extension
mechanisms.

## Normative model

### Stable identity

Every Ontology, Object Type, Property, and Link Type has an `id` with these
semantics:

- It is a canonical UUIDv7 generated once when the entity is created.
- It is opaque, immutable, and contains no kind, name, deployment, or provider
  information.
- References use IDs, never names.
- A deleted ID is never reused. Recreating an entity with the same name creates
  a different identity.
- Each ID has a Rust newtype and an OpenAPI UUID representation; an ID for one
  entity kind cannot be accepted where another kind is required.

The important borrowed distinction is identity versus name. Kubernetes, for
example, gives an object both a name scoped to a resource kind and a UID unique
over the cluster lifetime, and explicitly allows a deleted name to be reused
while the UID distinguishes the new occurrence from the old one
([Kubernetes: Object Names and IDs](https://kubernetes.io/docs/concepts/overview/working-with-objects/names/)).
UUIDv7 is standardized as a time-ordered UUID with random bits for uniqueness,
and RFC 9562 recommends it over UUIDv1 and UUIDv6 where possible
([RFC 9562, section 5.7](https://www.rfc-editor.org/rfc/rfc9562.html#section-5.7)).

The MVP should reject external resource identifiers, provider namespaces,
compatibility aliases, and type information encoded into an ID. They create a
second identity system without a current consumer.

### Names and labels

Each entity has a machine `name`, separate from its `display_name` and optional
`description`.

`name` is:

- mutable without changing `id`;
- 1 to 64 ASCII characters;
- lower snake case matching `^[a-z][a-z0-9_]{0,63}$`;
- compared byte-for-byte after validation, with no case folding or Unicode
  normalization;
- unique in the following scopes: Ontology names across the deployment, Object
  Type names within an Ontology, Link Type names within an Ontology, and
  Property names within their owning Object Type.

Object Type, Link Type, and Property names occupy separate namespaces. A name
is a machine handle, not durable identity; clients that retain a reference must
retain the ID. A rename that collides in its scope invalidates the whole save.

`display_name` is mutable, human-facing Unicode text, need not be unique, and
must contain at least one non-whitespace character. It is not used in URLs or
references. `description` is optional human-facing Unicode text.

The conservative ASCII identifier grammar follows the same interoperability
reasoning as GraphQL names: its specification restricts names to ASCII letters,
digits, and underscore, makes them case-sensitive, and notes that the ASCII
restriction supports interoperation
([GraphQL Specification, Names](https://spec.graphql.org/October2021/#sec-Names)).
The lowercase-only subset avoids visually distinct names that differ only by
case and gives generated clients one canonical spelling. The 64-character bound
is a local API limit, not a borrowed compatibility promise.

### Property ownership and value types

A Property is owned by exactly one Object Type. It is not a separately reusable
shared type in the MVP. Its `id` remains stable across renames and reordering.
The property order in a request is presentation data only and does not affect
schema meaning.

The complete MVP `value_type` enum is:

| Value | Meaning |
| --- | --- |
| `string` | A Unicode string |
| `integer` | A JSON number with zero fractional part |
| `number` | A finite JSON number |
| `boolean` | `true` or `false` |
| `date` | A string satisfying RFC 3339 `full-date` |
| `date_time` | A string satisfying RFC 3339 `date-time` |

JSON Schema defines `null`, `boolean`, `object`, `array`, `number`, and `string`
as primitive instance types and defines `integer` as a number with a zero
fractional part
([JSON Schema 2020-12 Validation, `type`](https://json-schema.org/draft/2020-12/json-schema-validation#name-type)).
It defines `date` and `date-time` formats by the corresponding RFC 3339 rules
([JSON Schema 2020-12 Validation, dates and times](https://json-schema.org/draft/2020-12/json-schema-validation#name-dates-times-and-duration)).

The MVP deliberately rejects `null`, array/list, nested object/struct, enum,
binary/attachment, geometry, vector, time-series, encrypted/marked values, and
generic JSON escape hatches. `null` would conflate presence with value;
collection and structured types require additional schema; and the specialized
types have no current end-to-end consumer. They can be added as explicit enum
variants when their validation and API representations are designed.

The logical value enum does not prescribe a future object-store column type.
In particular, it must not expose physical foreign-key, join-table, index, or
storage binding metadata.

### Requiredness

Every Property carries an explicit `required: boolean`; it is not silently
defaulted when omitted from an API request.

- `required: true` means every future Object instance has exactly one value for
  the Property.
- `required: false` means an instance has zero or one value.
- An explicit JSON `null` is never a value of any MVP Property type. Optional
  means absent, not nullable.
- Because the MVP stores no Object instances, this is a declared contract for a
  future data plane. The metadata service validates the declaration but has no
  instance values to check.

JSON Schema's `required` keyword constrains presence: an object is valid when
each listed property name occurs, independently of the property's value
([JSON Schema 2020-12 Validation, `required`](https://json-schema.org/draft/2020-12/json-schema-validation#name-required)).
SHACL similarly models lower and upper counts independently: `minCount` is the
minimum number of values and `maxCount` the maximum
([W3C SHACL, Cardinality Constraint Components](https://www.w3.org/TR/shacl/#core-components-count)).
The MVP specializes those general rules to scalar Properties with maximum one
and minimum zero or one.

### Link Type direction and cardinality

A Link Type is one named binary relation. It contains:

- `source_object_type_id`;
- `target_object_type_id`;
- `source_to_target`, either `one` or `many`;
- `target_to_source`, either `one` or `many`.

For a relation from source `S` to target `T`:

- `source_to_target: one` means each `S` may link to at most one `T`;
- `source_to_target: many` means each `S` may link to any number of `T` values;
- `target_to_source: one` means each `T` may be linked from at most one `S`;
- `target_to_source: many` means each `T` may be linked from any number of `S`
  values.

Thus the two fields express all four maximum-multiplicity combinations without
the usual perspective ambiguity of a single `one_to_many` label. Every minimum
is zero in the MVP: `one` means `0..1`, not exactly one, and `many` means
`0..*`. Required links and arbitrary numeric bounds are deferred.

Direction gives the relation a canonical source and target and gives the canvas
an arrow direction. It does not make the relationship one-way to query: the
same Link Type can be traversed from source to target or from target to source.
The reverse traversal is not stored as a second Link Type. Self-links and
multiple differently named Link Types between the same pair of Object Types
are valid.

OWL 2's narrow useful pattern is that an object property connects a pair of
individuals, has domain and range, and has a precisely defined inverse
([W3C OWL 2 Structural Specification, object properties](https://www.w3.org/TR/owl2-syntax/#Object_Properties)
and [inverse object properties](https://www.w3.org/TR/owl2-syntax/#Inverse_Object_Properties)).
Its minimum, maximum, and exact cardinality definitions also confirm that
cardinality is a count of distinct related individuals
([W3C OWL 2 Structural Specification, object cardinality](https://www.w3.org/TR/owl2-syntax/#Object_Property_Cardinality_Restrictions)).

The MVP must reject OWL's open-world inference semantics. The OWL specification
warns that domain/range axioms infer types rather than act as database-style
checks
([W3C OWL 2 Structural Specification, object-property domain](https://www.w3.org/TR/owl2-syntax/#Object_Property_Domain)).
Here, source and target are closed validation constraints: both referenced
Object Types must already occur in the submitted candidate.

### Deletion

The complete-schema request is authoritative. Within a successful save, an
existing child entity omitted from the candidate is hard-deleted.

- Removing a Property deletes that owned component.
- Removing a Link Type deletes that relation definition.
- Removing an Object Type is valid only if every incident Link Type is also
  absent from the same candidate. A remaining Link Type with a missing endpoint
  invalidates the entire save.
- Deleting an Ontology through its resource endpoint deletes the whole owned
  aggregate.
- A rename is an update, not delete-and-create, and therefore preserves ID.
- The MVP has no soft-delete flag, deprecation status, tombstone, restore,
  history, or lifecycle state.

The persistence model should use cascade only for composition (Ontology to its
owned schema rows, Object Type to its Properties). Link endpoints reference
independent Object Types and should be restrictive, with the service explicitly
deleting links before removed Object Types in the same transaction. PostgreSQL
documents that cascade is appropriate when the dependent cannot exist
independently, while restrict/no-action is appropriate between independent
objects
([PostgreSQL: Foreign Keys](https://www.postgresql.org/docs/current/ddl-constraints.html#DDL-CONSTRAINTS-FK)).

This hard-delete policy is safe only because the MVP stores metadata without
Object instances or historical schema versions. Introducing either consumer
creates a new migration and lifecycle decision; the MVP must not pretend to
solve that with an unused status column.

## Whole-schema validation and atomic save

The service validates the submitted candidate as one closed aggregate before
mutating persistence. Validation must accumulate a deterministic list of all
detectable violations rather than stopping at the first one. Each violation has
a stable machine code, a JSON-pointer-like path, and a safe human message. At a
minimum it checks:

1. every field and enum value is recognized; unknown fields are rejected;
2. IDs are canonical UUIDv7 values, are unique within their typed collections,
   and existing IDs have not changed kind or owner;
3. all names satisfy the grammar and are unique in their defined scope;
4. every Property belongs to exactly one Object Type and has a recognized value
   type plus explicit requiredness;
5. every Link Type's source and target IDs resolve to Object Types in the same
   candidate, and both directional cardinalities are recognized;
6. the Ontology identity in the candidate matches the resource being saved;
7. the expected revision matches the current revision.

An empty Ontology and an Object Type with zero Properties are valid. The MVP
does not require primary/title properties, infer missing types, repair dangling
references, or accept partially valid subgraphs.

If structural validation fails, the API returns the complete violation list and
makes no write. A revision mismatch is reported as a concurrency conflict and
also makes no write. If validation succeeds, the service rechecks the revision
at the persistence boundary, applies all inserts, updates, and deletions in one
PostgreSQL transaction, then increments the Ontology revision exactly once.
Readers observe either the old aggregate or the new aggregate, never an
intermediate graph. PostgreSQL transactions provide precisely this
all-or-nothing and visibility guarantee
([PostgreSQL: Transactions](https://www.postgresql.org/docs/current/tutorial-transactions.html)).

Database primary-key, unique, check, and foreign-key constraints duplicate the
critical storage invariants as a final safety boundary. PostgreSQL recommends
unique and foreign-key constraints for cross-row and cross-table invariants
rather than cross-row `CHECK` expressions
([PostgreSQL: Constraints](https://www.postgresql.org/docs/current/ddl-constraints.html)).
Application-level aggregate validation remains necessary for cross-collection
semantics and useful error paths.

SHACL's validation model is a useful precedent for reporting: a validation
report contains a conformance result plus validation results, and conforming
processors must be capable of returning all required results
([W3C SHACL, Validation Report](https://www.w3.org/TR/shacl/#validation-report)).
The MVP borrows the complete-report behavior, not SHACL's RDF representation.

## Explicitly rejected from the source draft

The source draft is useful as a list of hypotheses but is too broad for this
MVP. The specification should not include:

- any externally shaped resource ID or compatibility identifier;
- shared properties, interfaces, actions, groups, inheritance, or schema
  version snapshots;
- status/deprecation fields used as a substitute for an actual lifecycle;
- physical foreign-key or join-source bindings on Link Types;
- open-world inference, implicit inverse Link Types, or generic graph paths;
- storage-led value types that have no validated API representation;
- partial per-entity persistence during a canvas save.

Those exclusions preserve a small, coherent contract while keeping the stable
ID and aggregate boundaries needed for later extension.

## Sources

- [RFC 9562: Universally Unique IDentifiers](https://www.rfc-editor.org/rfc/rfc9562.html)
- [Kubernetes: Object Names and IDs](https://kubernetes.io/docs/concepts/overview/working-with-objects/names/)
- [GraphQL Specification: Names](https://spec.graphql.org/October2021/#sec-Names)
- [JSON Schema Draft 2020-12 Validation](https://json-schema.org/draft/2020-12/json-schema-validation)
- [W3C OWL 2 Structural Specification and Functional-Style Syntax](https://www.w3.org/TR/owl2-syntax/)
- [W3C Shapes Constraint Language (SHACL)](https://www.w3.org/TR/shacl/)
- [PostgreSQL: Constraints](https://www.postgresql.org/docs/current/ddl-constraints.html)
- [PostgreSQL: Transactions](https://www.postgresql.org/docs/current/tutorial-transactions.html)
