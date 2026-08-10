# Product Ontology metadata APIs and management boundaries

Research date: 2026-08-10

Ticket: [#59 — Research product ontology metadata APIs and management boundaries](https://github.com/time-gift24/stratum/issues/59)

## Question and evidence standard

What Ontology metamodel semantics and management boundaries are actually public in production products, and what can Stratum safely learn from them?

This report uses first-party product documentation, public API reference material, and first-party SDK source. Palantir Foundry is the primary subject. Microsoft Fabric Ontology is included as a genuinely comparable implementation, with an explicit warning that it is still in preview. The Palantir Python SDK is pinned to commit [`b8041668f871b1b54d4d25c24d623720d99881dc`](https://github.com/palantir/foundry-platform-python/tree/b8041668f871b1b54d4d25c24d623720d99881dc) so its evidence remains durable.

The user-provided pasted answer is audited claim by claim below. Secondary blog posts and unofficial documentation linked by that answer are not treated as evidence.

The report deliberately does **not** recommend Palantir compatibility, copy Palantir wire identifiers into Stratum, or treat a public response model as evidence of a vendor's private database schema.

## Answer in brief

1. **Yes: Object Type, Property, and Link Type should be separate first-class persistence tables in Stratum.** Property is not a JSON field hidden inside the Object Type row; it is an owned metamodel entity with its own stable ID and lifecycle. The current whole-schema API is an aggregate transfer and atomic-save boundary, not a claim that the aggregate is stored in one `schema` column or JSON document.
2. Public product evidence consistently separates **semantic definitions** from **physical data binding** and from **object instances**. A semantic Link Type can therefore carry its two endpoints and per-direction cardinalities without also carrying a datasource foreign key or join-table binding.
3. Palantir local properties are first-class. A shared property is an optional metadata association that an existing local property can attach to and detach from; it is not a mandatory indirection for every property.
4. Palantir publicly exposes a rich read projection, but calls its full-metadata endpoint preview, says it returns “as much metadata as possible,” and warns that it may omit entities. That projection is not a complete internal OMS schema and does not reveal OMS persistence.
5. Palantir does expose schema mutation outside Ontology Manager: current Palantir MCP tools create, update, and delete object, link, and action types on a branch, and Palantir documents an ontology-as-code path. The narrower generated Platform SDK metadata resources are read-oriented; that does not establish an intentional product-wide mutation boundary.
6. Branches, proposals, rebase, and merge are observable product workflows. Their private persistence representation is not public. There is no primary evidence for per-row `ontology_version_id`, snapshot-copy storage, or a `propose / merge / get(version)` service contract.
7. Stratum's present `revision`/ETag is a concurrency token, not a history version. Shared properties, interfaces, actions, groups, datasource bindings, object instances, and version workflows should remain outside this MVP instead of being prebuilt from speculative vendor internals.

## Keep four layers separate

| Layer | Public product meaning | Examples | Stratum MVP consequence |
| --- | --- | --- | --- |
| Semantic metamodel | Definitions and graph constraints | Ontology, Object Type, Property, Link Type; later shared properties and interfaces | Canonical normalized metadata tables; this is the current module |
| Physical binding | How semantic definitions map to concrete source data | Datasource, source column mapping, foreign-key mapping, join datasource | Separate future capability; do not put binding columns in the core Link Type table |
| Instance/data plane | Values and edges for concrete objects | Indexed objects, links, edits, search indexes, instance graph | Out of scope; do not infer its storage from metadata APIs |
| Change governance | How a candidate metamodel becomes accepted | Atomic replacement, branch, proposal, review, merge, history | MVP uses atomic replacement plus revision CAS only; workflow/history remains deferred |

Palantir's architecture makes the first three boundaries explicit. Its Ontology Metadata Service (OMS) defines ontological entities, while object databases store indexed object data, Object Set Service serves reads, Actions applies object edits, and Funnel orchestrates writes from datasources and edits ([Ontology architecture](https://www.palantir.com/docs/foundry/object-backend/overview/)). Microsoft Fabric similarly distinguishes ontology definitions, data binding, and the materialized instance graph ([Fabric Ontology overview](https://learn.microsoft.com/en-us/fabric/iq/ontology/overview)).

## Palantir: what is public

### Architecture and storage boundary

The official architecture says OMS defines object-type, link-type, action-type, and other metadata. It does not call OMS a relational database or publish its tables. “Defines the set of ontological entities” supports treating it as an authority for metadata semantics; it does not prove the stronger implementation claim “one relational schema is the sole source of truth” ([Ontology architecture](https://www.palantir.com/docs/foundry/object-backend/overview/)).

Object Storage v2 is described as the next-generation canonical store for indexed **object data**, not for OMS metadata. Palantir describes a backend composed of multiple services and object databases, but does not disclose a database topology or concrete OMS/OSv2 storage technologies. The public material therefore does not support claims such as “Cassandra plus Elasticsearch” or a particular parallel-database layout. For the legacy OSv1 path, Palantir does state that behavior was tightly coupled to an underlying distributed document store and search engine; it does not name those products ([Indexing FAQ](https://www.palantir.com/docs/foundry/object-indexing/faq/)). Palantir also says OSv1 will be unavailable after 2026-06-30 ([Ontology architecture](https://www.palantir.com/docs/foundry/object-backend/overview/)).

The transferable lesson is a boundary, not a storage design: metamodel persistence, physical source binding, and instance/index storage solve different problems.

### Public metadata is a consumer projection, not the private OMS schema

The `GET /api/v2/ontologies/{ontology}/fullMetadata` endpoint returns object, link, action, query, interface, shared-property, branch, and value-type metadata. However, the endpoint is in preview, is designed to return “as much metadata as possible,” and may omit entities rather than fail ([Get Ontology Full Metadata](https://www.palantir.com/docs/foundry/api/ontologies-v2-resources/ontologies/get-ontology-full-metadata/)). It is consequently false to describe this API as a complete field-level publication of the OMS internal schema.

The generated Python SDK is useful evidence for the public wire contract:

- [`ObjectTypeV2`](https://github.com/palantir/foundry-platform-python/blob/b8041668f871b1b54d4d25c24d623720d99881dc/foundry_sdk/v2/ontologies/models.py#L3970-L4001) exposes API name, display metadata, status, primary/title properties, a property map, RID, and optionally visible datasources.
- [`PropertyV2`](https://github.com/palantir/foundry-platform-python/blob/b8041668f871b1b54d4d25c24d623720d99881dc/foundry_sdk/v2/ontologies/models.py#L4701-L4712) exposes a property RID, data type, display metadata, visibility, value-type name, formatting, and type classes. It is not an exhaustive copy of every Ontology Manager concern; for example, required-property configuration is documented separately ([Required properties](https://www.palantir.com/docs/foundry/object-link-types/required-properties/)).
- [`LinkTypeSideV2`](https://github.com/palantir/foundry-platform-python/blob/b8041668f871b1b54d4d25c24d623720d99881dc/foundry_sdk/v2/ontologies/models.py#L2407-L2419) exposes a side API name, object-type API name, `ONE`/`MANY` cardinality, link RID, and an optional foreign-key property API name.
- The generated [`ObjectType` resource client](https://github.com/palantir/foundry-platform-python/blob/b8041668f871b1b54d4d25c24d623720d99881dc/foundry_sdk/v2/ontologies/object_type.py#L60-L567) contains get/list/full-metadata/history/link-read operations, not type-schema CRUD. This is evidence about that SDK surface only.

SDK generation demonstrates that the wire projection is machine-readable. It does not establish internal table layout, field completeness, validation ownership, or a safe schema to copy wholesale.

### Mutations and governance are exposed through more than one surface

The claim that Palantir deliberately withholds schema mutation and permits changes only through Ontology Manager is contradicted by current first-party documentation:

- Palantir MCP lists tools to create/update/delete object types, link types, and action types on a supplied branch, plus tools to create branches and proposals ([Palantir MCP available tools](https://www.palantir.com/docs/foundry/palantir-mcp/available-tools/)).
- Palantir distinguishes Ontology MCP, which lets consumers read objects and execute controlled actions against ontology data, from Palantir MCP, which lets builders modify ontology types but cannot write actual ontology data ([Ontology MCP overview](https://www.palantir.com/docs/foundry/ontology-mcp/overview/)). That distinction is another direct example of metadata-plane versus data-plane boundaries.
- Palantir documents SuperRepos as an ontology-as-code path for programmatic object-type creation ([Create an object type](https://www.palantir.com/docs/foundry/object-link-types/create-object-type/)).

Branches and proposals are real, but conditional. Object, action, link, interface, and shared-property types can be protected; when protection is enabled, edits must use a branch and proposal. Type groups are not branch-protected. Unprotected changes may be saved directly ([Branching the ontology](https://www.palantir.com/docs/foundry/ontologies/branching-ontology/)). Thus “every schema change must be proposal → approval → merge” is too broad.

Deletion also exists. An active resource cannot be deleted or API-renamed, but it can be moved to experimental or deprecated status and then deleted; experimental API names may change. Palantir has active, experimental, deprecated, example, and promoted status semantics, not only `ACTIVE / DEPRECATED` ([Metadata statuses](https://www.palantir.com/docs/foundry/object-link-types/metadata-statuses/)).

### Identity is richer than `rid + api_name`

Palantir documents Object Type **ID**, **RID**, **API name**, and display name as distinct metadata ([Object Type metadata](https://www.palantir.com/docs/foundry/object-link-types/object-type-metadata/)). The public full-metadata API accepts an ontology API name or RID, and its response maps are often keyed by API names. Link-side references in the SDK use object and property API names while also returning a link RID. Therefore:

- “Palantir has only RID and API name” is incomplete.
- “All foreign keys use RID and never API name” is contradicted by the public contract.
- No public identifier behavior proves how OMS database foreign keys are physically stored.

The transferable invariant is narrower: use immutable internal identity separately from mutable, validated symbolic names. Stratum already satisfies this with typed UUIDv7 IDs and scoped names; it does not need Palantir RIDs.

### Object Type and Property

Palantir Object Types are concrete instance-bearing types. The Palantir creation workflow consequently requires at least one Property plus primary and title keys, and configures datasource mappings ([Create an object type](https://www.palantir.com/docs/foundry/object-link-types/create-object-type/)). Those requirements come from Palantir's instance/data-plane contract. They are not universal metamodel requirements and should not be imported into Stratum's current schema-only MVP, where an empty Object Type is intentionally valid.

A Palantir shared property centralizes metadata that can be used on multiple Object Types; the underlying values remain separate ([Shared properties](https://www.palantir.com/docs/foundry/object-link-types/shared-property-overview/)). Crucially, attaching a local Property to a shared property preserves the local Property ID and API name, inherited metadata becomes read-only, local and shared type classes are combined, and the association can later be detached ([Use shared properties](https://www.palantir.com/docs/foundry/object-link-types/use-shared-property/)). This establishes:

- A local Property remains a first-class identity-bearing entity.
- Shared-property use is optional.
- If Stratum later adds shared properties, the faithful semantic shape would be an optional association from a local Property to a shared definition plus explicit inheritance/override rules—not a mandatory shared-property row for every Property.

It does **not** establish a particular relational table design for Palantir.

### Link Type, directionality, cardinality, and binding

A Palantir Link Type is one schema definition between two Object Types. It has two independently traversable sides, each with its own API/display name; a second reverse Link Type is not created ([Link types](https://www.palantir.com/docs/foundry/object-link-types/link-types-overview/)). This supports Stratum's one-Link-Type/two-directions model.

Palantir can physically define links through Object Type foreign keys for one-to-one/many-to-one, a join-table datasource for many-to-many, or a backing Object Type. It explicitly says one-to-one is an expression of intent and is not enforced ([Create a link type](https://www.palantir.com/docs/foundry/object-link-types/create-link-type/)). These are product binding choices, not proof that every semantic Link Type table must contain `fk_property_id` or join-column IDs.

The important distinction is:

- Semantic cardinality says how many targets are permitted/expected from each side.
- Physical binding says how instance edges are derived or stored.

Stratum should persist the former now and defer the latter until physical data integration or object instances enter scope.

### Interfaces

Palantir Interfaces are abstract contracts over properties, link constraints, and action constraints; Object Types are concrete and instance-bearing. An Object Type may implement multiple Interfaces, and an Interface may extend multiple Interfaces through multiple layers ([Interfaces](https://www.palantir.com/docs/foundry/interfaces/interface-overview/)). An implementation maps existing local Properties to required Interface Properties and may also map link/action constraints ([Implement an interface](https://www.palantir.com/docs/foundry/interfaces/implement-interface/)).

Multiple inheritance is verified. The public documentation reviewed does not state the internal persistence shape or an explicit cycle-prevention invariant. Modeling future extension edges as a DAG is a reasonable Stratum design proposal, but it must be labelled as our invariant rather than a Palantir fact.

### Actions

Palantir Action Types have parameters, submission criteria, permission/validation concerns, rules that create/modify/delete objects and links, function-backed rules, notifications, and webhooks ([Action rules](https://www.palantir.com/docs/foundry/action-types/rules/), [Side effects](https://www.palantir.com/docs/foundry/action-types/side-effects-overview/)). An Action's object edits are described as one transaction ([Object edits](https://www.palantir.com/docs/foundry/object-edits/overview/)), but that guarantee does not make external effects atomic: writeback can succeed before a later Ontology change fails, while side-effect webhooks run after object changes, may complete after user success, and have no guaranteed ordering ([Webhooks](https://www.palantir.com/docs/foundry/action-types/webhooks/)).

These public semantics do not reveal whether Palantir stores parameters/rules/effects in JSONB, normalized tables, event definitions, or another representation. A proposed `parameters JSONB`, `rules JSONB`, `side_effects JSONB`, and single `function_rid` row shape is therefore speculative and too early for Stratum's MVP.

### Groups

Palantir Object Type Groups are classification resources for search and exploration. They are managed resources with project viewer permission, not merely free-form display strings ([Object Type groups](https://www.palantir.com/docs/foundry/object-link-types/type-groups/)). They still do not belong in Stratum's confirmed MVP.

### Version and workflow representation

Palantir publicly documents branch creation, proposals for protected resources, rebase, conflict resolution, and merge. The metadata read API optionally accepts a branch, while warning that branch support is experimental and not supported by all workflows ([Get Ontology Full Metadata](https://www.palantir.com/docs/foundry/api/ontologies-v2-resources/ontologies/get-ontology-full-metadata/), [Branching the ontology](https://www.palantir.com/docs/foundry/ontologies/branching-ontology/)).

No reviewed primary source specifies:

- an `ontology_version` relation;
- `ontology_version_id` on every metadata row;
- full snapshot copying on merge;
- the size of an average metamodel or that copy amplification is negligible;
- a public `propose / merge / get(version)` service boundary.

Those may be viable future Stratum designs, but they cannot be attributed to Palantir. In this MVP, `revision` is only a strong compare-and-swap token for atomic replacement. Treating it as a historical version would silently expand the product contract.

## Comparable implementation: Microsoft Fabric Ontology (Preview)

Microsoft Fabric Ontology independently reinforces the layer separation, but all findings in this section carry its current **Preview** status.

Fabric describes Entity Types, Properties, and Relationship Types as definitions; Data Binding connects those definitions to concrete OneLake tables/streams/models; the Ontology graph contains materialized entity and relationship instances ([Fabric Ontology overview](https://learn.microsoft.com/en-us/fabric/iq/ontology/overview)).

Its documented item-definition package makes the separation structural:

- `EntityTypes/{ID}/definition.json` contains entity and Property definitions.
- `EntityTypes/{ID}/DataBindings/...` separately maps source columns to target Property IDs and identifies a concrete Lakehouse/Eventhouse table.
- `RelationshipTypes/{ID}/definition.json` contains the semantic source and target.
- Relationship contextualizations separately bind source and target keys to concrete data.

The same schema supports `entityIdParts` as one or more Property IDs, so composite instance identity is a real product choice rather than a universal one-property-key law ([Ontology item definition](https://learn.microsoft.com/en-us/rest/api/fabric/articles/item-management/definitions/ontology-definition)). Fabric's generic item API replaces the item definition as a permissioned whole definition and can run asynchronously ([Update Item Definition](https://learn.microsoft.com/en-us/rest/api/fabric/core/items/update-item-definition)). This shows that a whole-definition wire mutation can coexist with separately structured definition parts; it still does not reveal the service's database tables.

## Audit of the pasted answer

Legend: **Verified** means primary sources directly support the narrow claim; **Partially verified** means only part is supported or the scope was overstated; **Contradicted** means primary sources show a conflicting behavior; **Unsupported** means no reviewed primary source establishes it.

| # | Material claim from the pasted answer | Finding | Evidence and correction |
| --- | --- | --- | --- |
| 1 | OMS defines Object, Link, Action, Interface, shared-property, and other ontology metadata. | **Verified, narrowly** | OMS officially defines the set of ontological entities, including Object, Link, and Action Types; the public metadata surfaces also expose Interfaces and shared properties. “Single source of truth” is a plausible characterization, not the exact published storage contract ([architecture](https://www.palantir.com/docs/foundry/object-backend/overview/)). |
| 2 | REST/Conjure publishes a complete field-level OMS schema. | **Contradicted** | Full metadata is preview, aims to return as much as possible, and may omit entities ([API](https://www.palantir.com/docs/foundry/api/ontologies-v2-resources/ontologies/get-ontology-full-metadata/)). |
| 3 | Because OSDK is generated, copying the public model will not go wrong. | **Unsupported inference** | Generation verifies a public wire projection, not internal completeness, invariants, persistence, or suitability for Stratum. |
| 4 | Object/Link/Action schema mutation is intentionally unavailable; only Ontology Manager can mutate it. | **Contradicted** | Palantir MCP exposes create/update/delete type tools on branches, and SuperRepos provide ontology-as-code ([MCP tools](https://www.palantir.com/docs/foundry/palantir-mcp/available-tools/), [object creation](https://www.palantir.com/docs/foundry/object-link-types/create-object-type/)). The generated Platform SDK read resources are only one surface. |
| 5 | All schema changes must follow branch → proposal → approval → merge. | **Contradicted as universal** | The workflow is required for protected resources; unprotected resources may save directly, and Type Groups are not protected ([branching](https://www.palantir.com/docs/foundry/ontologies/branching-ontology/)). |
| 6 | OSv2 is backed by “multiple specialized object databases in parallel.” | **Partially verified** | Palantir documents multiple backend services/object databases and OSv2 as canonical object-data storage, but not that precise topology or phrase ([architecture](https://www.palantir.com/docs/foundry/object-backend/overview/)). |
| 7 | Cassandra, Elasticsearch, KV, or graph-index technology is undisclosed. | **Verified only as an evidence limit** | No reviewed first-party source names those products for OSv2/OMS. Naming any of them is speculation; absence of public evidence is not proof of internal absence. |
| 8 | OSv1 was tightly coupled to a distributed document store and search engine and is unavailable after 2026-06-30. | **Verified** | Both statements appear in Palantir's official architecture/indexing documentation ([FAQ](https://www.palantir.com/docs/foundry/object-indexing/faq/), [architecture](https://www.palantir.com/docs/foundry/object-backend/overview/)). |
| 9 | Copy metamodel semantics, not OSv2 storage. | **Supported design lesson** | Metadata and indexed object storage are separate public components. “Copy,” however, should mean learning boundary/invariants, not copying product names or wire shapes. |
| 10 | Build a versioned relational “OMS equivalent.” | **Unsupported as a product fact** | Relational OMS storage and version-row layout are not public. It is a local option, and versioning is explicitly outside the Stratum MVP. |
| 11 | The proposed ER skeleton reflects Palantir's internal schema. | **Unsupported** | The entity concepts are public; the table boundaries, foreign keys, JSONB, and version columns are not. |
| 12 | Every metadata entity has exactly RID + API name, and all internal foreign keys use RID. | **Contradicted/incomplete** | Palantir also distinguishes ID and display name; public references use both API names and RIDs ([metadata](https://www.palantir.com/docs/foundry/object-link-types/object-type-metadata/), [SDK model](https://github.com/palantir/foundry-platform-python/blob/b8041668f871b1b54d4d25c24d623720d99881dc/foundry_sdk/v2/ontologies/models.py#L2407-L2419)). Internal database foreign keys are unknown. |
| 13 | Every Property is a shared-property binding plus local overrides. | **Contradicted** | A local Property can exist without a shared property; attachment/detachment preserves local identity ([shared-property use](https://www.palantir.com/docs/foundry/object-link-types/use-shared-property/)). |
| 14 | `properties` should not be a separate local table. | **Contradicted by the semantics and Stratum's requirement** | Local Properties are identity-bearing, owned entities. In Stratum they should be separate rows owned by an Object Type; a future shared definition would be optional. |
| 15 | Link Type must store an FK Property or join-source/join-target Property IDs in its semantic row. | **Unsupported and layer-mixing** | Palantir supports FK, join-table, and backing-object binding choices, but those describe physical instance binding ([create Link Type](https://www.palantir.com/docs/foundry/object-link-types/create-link-type/)). Semantic endpoints/cardinalities can stand alone. |
| 16 | Link Type is bidirectional and cardinality exists on its sides. | **Verified** | One Link Type has two independently traversable sides; the public SDK models per-side `ONE`/`MANY` cardinality ([links](https://www.palantir.com/docs/foundry/object-link-types/link-types-overview/), [SDK](https://github.com/palantir/foundry-platform-python/blob/b8041668f871b1b54d4d25c24d623720d99881dc/foundry_sdk/v2/ontologies/models.py#L2407-L2419)). |
| 17 | Interface inheritance is a multiple-inheritance DAG. | **Partially verified** | Multiple extension and multiple implementation are documented. Acyclicity/internal persistence was not found in public docs ([interfaces](https://www.palantir.com/docs/foundry/interfaces/interface-overview/)). |
| 18 | Interface implementation is just a join row and “has all required properties.” | **Incomplete** | Palantir requires explicit mappings for local Properties and may require Link/Action constraint mappings ([implementation](https://www.palantir.com/docs/foundry/interfaces/implement-interface/)). |
| 19 | Action parameters, rules, criteria, effects, and one function reference should be JSONB columns. | **Unsupported storage proposal** | Those semantic concerns exist, but their private storage representation does not. Function-backed rules also have exclusivity semantics that a loose JSONB suggestion does not encode ([rules](https://www.palantir.com/docs/foundry/action-types/rules/)). |
| 20 | An Action is simply one transaction, including its side effects. | **Contradicted if applied to external effects** | Object edits are transactional, but webhook writebacks/side effects have failure and ordering gaps ([object edits](https://www.palantir.com/docs/foundry/object-edits/overview/), [webhooks](https://www.palantir.com/docs/foundry/action-types/webhooks/)). |
| 21 | Branch merge materializes a full copied snapshot; every row carries `ontology_version_id`. | **Unsupported** | Public workflow behavior does not disclose persistence. |
| 22 | Metadata is so small that snapshot-copy amplification never matters. | **Unsupported performance assumption** | No primary evidence or Stratum workload bound was supplied. |
| 23 | The exact DDL's `ACTIVE / DEPRECATED` status model matches Palantir. | **Contradicted** | Palantir also has experimental, example, and promoted statuses, with status-dependent rename/delete rules ([statuses](https://www.palantir.com/docs/foundry/object-link-types/metadata-statuses/)). |
| 24 | Palantir does not hard-delete resources. | **Contradicted** | Active resources must first change status; experimental/deprecated resources can then be deleted ([statuses](https://www.palantir.com/docs/foundry/object-link-types/metadata-statuses/)). |
| 25 | The listed property-type enum is an authoritative complete copy. | **Unsupported/inaccurate** | The current public SDK models a larger discriminated union and separates several concepts; Stratum's six scalar types are an intentional local scope, not a Palantir subset contract ([SDK data types](https://github.com/palantir/foundry-platform-python/blob/b8041668f871b1b54d4d25c24d623720d99881dc/foundry_sdk/v2/ontologies/models.py#L4056-L4084)). |
| 26 | Every Object Type universally requires a primary/title Property. | **Product-specific, not transferable** | Palantir requires these because its Object Types are instance-bearing and datasource-backed. Fabric permits one or more identity parts. Stratum currently has no instances, so empty Object Types remain valid. |
| 27 | Object Type Groups are a pure display layer. | **Contradicted/incomplete** | They support classification, search/discovery, and project permission checks ([groups](https://www.palantir.com/docs/foundry/object-link-types/type-groups/)). |
| 28 | Stratum should expose `propose / merge / get(version)` immediately. | **Unsupported and out of scope** | No public Palantir service contract establishes this; it would add deferred workflow/history semantics to the MVP. |
| 29 | Instance storage should be a separate later decision. | **Verified boundary** | Both Palantir and Fabric separate semantic metadata from indexed/materialized object data. KV/index/adjacency choices remain unevidenced and should not be preselected. |

## Recommended Stratum metadata boundary

This is a scope decision, not a Palantir compatibility design.

### Canonical normalized persistence

| Canonical relation | Ownership and minimum semantic responsibility | Explicitly not in the relation now |
| --- | --- | --- |
| `ontologies` | Aggregate root, immutable ID, scoped name/display metadata, current revision | History snapshots, branch/proposal state, instance storage |
| `object_types` | One row per Object Type, owned by one Ontology, with immutable typed ID and mutable validated names/description | Embedded Property JSON, datasource, primary/title instance keys |
| `properties` | One row per local Property, owned by one Object Type, with immutable typed ID, name/display metadata, and one of the six MVP scalar value types | Mandatory shared-property indirection, nullability/required instance semantics, source-column binding |
| `link_types` | One row per binary Link Type, owned by one Ontology, referring to two Object Types in that Ontology, with independently stored `one`/`many` cardinality for each direction | FK column binding, join datasource, link instances, reverse duplicate Link Type |
| canvas layout relation | Presentation coordinates keyed by metamodel entity ID if persisted with the aggregate | Domain semantics or identity |

The whole candidate graph sent by `PUT` remains the **API aggregate** and transaction boundary. The repository can validate the complete closed graph and replace its normalized child rows inside one PostgreSQL transaction guarded by the Ontology revision. Calling that payload “schema” means “the complete semantic definition,” not “one database schema table” and not “one JSONB schema column.”

### Invariants justified now

- Object Type, Property, and Link Type IDs are stable typed UUIDv7 values; names are mutable and unique in their documented scopes.
- Each Property belongs to exactly one Object Type. Deleting an Object Type removes its owned Properties only when the complete candidate graph is valid.
- Each Link Type belongs to one Ontology and references two Object Types from that same Ontology; self-links are allowed if the existing contract allows them.
- A Link Type is one relationship with two directions, not two reverse records. Each direction stores `one` or `many` independently.
- A whole-schema save is atomic and guarded by strong ETag/revision compare-and-swap. Revision is not exposed as history.
- An empty Ontology and an Object Type with zero Properties remain valid because this module defines a schema graph, not object instances.

### Defer without speculative placeholders

- **Shared properties:** later add a separate shared definition and optional local-Property association only when cross-type metadata reuse is required.
- **Interfaces:** later add interface contracts, extension edges, implementation mappings, and cycle validation together; a simple implements join alone is insufficient.
- **Actions:** later model an operational command contract with explicit transaction and external-effect semantics; do not reserve opaque JSONB merely because a vendor UI has complex forms.
- **Groups:** later add a classification resource and membership/permission semantics if discovery needs it.
- **Physical binding:** later add datasource and mapping resources separate from semantic types.
- **Version workflows:** later define history, branch, proposal, merge, rollback, and audit requirements from Stratum use cases. Do not overload the current revision token.
- **Object instances:** keep storage, indexing, primary identity, edits, and query APIs in a separate future data-plane design.

## Conclusion

The public evidence supports the user's correction: `object_types`, `properties`, and `link_types` are distinct first-class semantic resources and should be distinct canonical tables. It does **not** support turning the public response model into a vendor-shaped relational schema, requiring every Property to route through a shared definition, embedding physical link bindings in the semantic Link Type, or introducing branch/version tables now.

The clean Stratum boundary is therefore: a normalized semantic metadata core with an aggregate whole-schema API, strict closed-graph validation, and atomic revision-guarded replacement. Physical bindings, instances, rich reusable abstractions, actions, groups, and version governance remain separate capabilities that can be added when their own requirements exist.
