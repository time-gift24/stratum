# Neo4j native LPG versus n10s-mapped RDF for the ABox

Research date: 2026-08-16

## Question and decision frame

Stratum has accepted three architectural boundaries for the next Ontology
iteration:

- `Ontology` owns a Stratum-defined TBox and has a lifecycle independent from
  the ABox-owning `Knowledge Base`;
- the ABox is separated logically into Asserted ABox and Derived ABox;
- SHACL is initially the detection contract, while materializing a Derived ABox
  is a separate execution contract.

This note compares two meanings of “use Neo4j for the ABox”:

1. **Native LPG canonical model** — Stratum defines nodes, labels,
   relationships, properties, identity, partitions, and provenance directly in
   Neo4j's Labeled Property Graph model. RDF is an import/export projection.
2. **RDF Dataset canonical model mapped by n10s** — Stratum treats RDF terms,
   triples, and graph membership as authoritative, and neosemantics (`n10s`)
   maps that data into Neo4j's LPG structures.

The second option is not “Neo4j running as an RDF store.” Neo4j describes n10s
as an extension for importing/exporting RDF, SHACL validation, and basic
inference; the n10s reference explicitly says RDF import stores the data “as a
property graph.” Queries over the stored data remain Cypher queries over the
mapped LPG. [Neo4j: neosemantics](https://neo4j.com/labs/neosemantics/)
[n10s reference: RDF import](https://neo4j.com/labs/neosemantics/5.14/reference/#_rdf_import)

This distinction is the core decision. In option 1, the LPG shape is the
contract. In option 2, the RDF Dataset is the contract and every n10s
configuration choice is a persistence codec whose fidelity must be proven.

## Evidence convention

Statements tied directly to W3C specifications, Neo4j documentation, or the
tagged n10s source are documented facts. Paragraphs explicitly introduced as
an “architectural inference,” all option trade-offs, and the recommendation are
reasoned consequences for Stratum rather than vendor guarantees. No community
posts or third-party comparisons are used as evidence.

## Findings in brief

- Native LPG gives Stratum the shortest operational path: natural Cypher,
  direct Neo4j constraints and transactions, Aura compatibility, and no server
  plugin lifecycle. It does **not** provide RDF's global IRI identity, literal
  term model, named graphs, or statement semantics unless Stratum designs them.
- n10s preserves RDF resource URIs and can retain vocabulary namespaces,
  multi-values, language tags, and custom datatypes, but its defaults do not
  preserve all of them. In particular, the default `OVERWRITE` mode retains
  only the last literal value; language tags and custom datatypes are disabled
  by default. Fidelity is a GraphConfig policy, not an unconditional property.
  [n10s reference: Graph Config](https://neo4j.com/labs/neosemantics/5.14/reference/#_graph_config_params_global_settings)
- n10s named-graph import is an `n10s.experimental.quadrdf.*` feature. Its
  source represents the same RDF IRI in two named graphs as two Neo4j nodes,
  keyed by `(uri, graphUri)`. This is workable as a graph-local projection, but
  it means Asserted/Derived graph partitioning changes Neo4j node identity and
  makes cross-partition queries less natural.
  [n10s migration appendix](https://github.com/neo4j-labs/neosemantics/blob/2025.06.1/docs/modules/ROOT/pages/appendix_migration.adoc)
  [n10s quad mapper source](https://github.com/neo4j-labs/neosemantics/blob/2025.06.1/src/main/java/n10s/quadrdf/RDFQuadToLPGStatementProcessor.java)
- n10s SHACL is a useful partial compiler, not a general SHACL engine. Official
  documentation states that it implements a significant portion, not all, of
  SHACL. Tagged source supports a concrete allowlist of targets, direct/inverse
  paths, and constraint components; it does not implement the complete SHACL
  Core surface, SHACL-SPARQL, or SHACL-AF Rules.
  [n10s SHACL documentation](https://neo4j.com/labs/neosemantics/4.0/validation/)
  [n10s validator source](https://github.com/neo4j-labs/neosemantics/blob/2025.06.1/src/main/java/n10s/validation/SHACLValidator.java)
- n10s “basic inference” is query-time hierarchy expansion through functions
  and procedures. It does not provide OWL 2 RL closure or Stratum's required
  Derived ABox materialization and lineage.
  [n10s inferencing](https://neo4j.com/labs/neosemantics/5.14/inference/)
- n10s is available only on self-hosted Neo4j CE/EE, not Aura. As of the
  research date, its latest published release is `2025.06.1`; that release's
  build targets Neo4j `2025.06.2`, while Neo4j's current documentation covers
  the 2026 series. Adopting n10s as a required semantic boundary therefore adds
  a server/plugin compatibility gate.
  [n10s availability](https://neo4j.com/labs/neosemantics/#availability-installation)
  [n10s releases](https://github.com/neo4j-labs/neosemantics/releases)
  [n10s 2025.06.1 build](https://github.com/neo4j-labs/neosemantics/blob/2025.06.1/pom.xml)
- Neo4j lists Rust's `neo4rs` as a community driver rather than an officially
  supported driver. This risk exists for both storage shapes. n10s adds dynamic
  procedure result handling and plugin-specific error contracts on top of that
  common driver risk.
  [Neo4j Bolt: official and community drivers](https://neo4j.com/docs/bolt/current/neo4j-drivers/)

## Decision matrix

| Concern | Native LPG is canonical | RDF Dataset is canonical, mapped by n10s |
| --- | --- | --- |
| Identity | Stratum must generate stable IDs and constrain them; Neo4j internal element IDs are unsuitable outside one transaction | RDF IRIs supply global identity in the logical model; ordinary n10s import uses unique `:Resource(uri)` nodes |
| Namespace semantics | Labels/types/property keys have only application-defined naming scopes | Full vocabulary IRIs can be kept, shortened, mapped, or discarded; only `KEEP`/disciplined prefixing retains namespace distinction directly |
| Literals | Neo4j-native scalar and list values; simple for the product, but not RDF term equality | Mapping may collapse RDF lexical/datatype distinctions; language/custom datatype fidelity requires non-default configuration |
| Multi-values | Choose relationship, array property, or assertion node per domain rule | RDF repeated triples map to arrays only with `handleMultival: ARRAY`; default `OVERWRITE` loses earlier values |
| Dataset partitions | Must model graph/partition membership explicitly | Standard named graphs exist logically, but n10s quad support is experimental and duplicates resource nodes by graph |
| Asserted vs Derived | Explicit `origin`, graph membership, or assertion nodes; easy to query in one connected graph | Natural as two named graphs in RDF, but n10s's physical mapping fragments node identity; alternative explicit provenance triples still need a model |
| Statement provenance | Native relationship properties help object-object assertions; literal-property assertions need reification/assertion nodes | RDF named graphs provide graph-level context; statement-level metadata needs RDF 1.2 reifiers/reification or an assertion vocabulary, and n10s's tagged RDF-star mapping is incomplete |
| SHACL | Requires a Stratum compiler/validator or n10s used only as a partial LPG validator | Shapes are native RDF, but n10s implements only a subset and returns Neo4j-oriented validation rows |
| Reasoning/materialization | Stratum owns a deterministic materializer and lineage model | Still required: n10s only offers basic query-time hierarchy inference |
| Query ergonomics | Domain-oriented labels and relationships produce concise Cypher | Still Cypher, plus prefix/URI escaping, `Resource` nodes, array handling, and graph-context predicates |
| Transactions/constraints | Full Neo4j ACID transactions and native identity/type/existence constraints, edition permitting | Same physical transaction guarantees; n10s adds a required resource constraint/config and uses periodic commits for large import by default |
| Bulk ingestion | Native admin CSV/Parquet import and Cypher batching | Direct RDF parsing with configurable periodic commits; convenient for RDF but not as broad as Neo4j's native bulk-import paths |
| Deployment | Self-hosted or Aura; no semantic server plugin | Self-hosted CE/EE only; version-compatible n10s JAR and optional APOC/HTTP extension management |
| Rust | Community `neo4rs` or HTTP Query API; typed repository code is owned by Stratum | Same, plus calls to dynamically typed n10s procedures; there is no official Rust RDF/n10s client contract |
| Export/portability | Requires a deliberately maintained RDF exporter/mapping | Stronger RDF interchange story when the mapping is configured losslessly; named-graph and RDF 1.2 fidelity need separate proof |
| Lock-in | Neo4j model and Cypher are the canonical contract | Logical RDF lowers data-model lock-in, but the physical mapping and runtime behavior remain n10s/Neo4j-specific |

## Detailed comparison

### 1. Identity and namespace semantics

RDF IRIs have global scope: two appearances of the same IRI denote the same
resource. Namespace prefixes are only serialization conveniences; expanded
IRIs are the terms that participate in identity. RDF literals and IRIs remain
different terms even if based on the same string.
[W3C RDF 1.1 Concepts: IRIs](https://www.w3.org/TR/rdf11-concepts/#section-IRIs)
[W3C RDF 1.1 Concepts: literals](https://www.w3.org/TR/rdf11-concepts/#section-Graph-Literal)

Neo4j nodes have internal element IDs, but Neo4j guarantees their mapping only
within one transaction and can reuse internal IDs after deletion. Its own
manual recommends application-generated IDs. A native LPG ABox must therefore
use stable Stratum newtypes—such as `KnowledgeEntityId` and `AssertionId`—as
properties protected by uniqueness/key constraints. Labels, relationship
types, and property keys are case-sensitive identifiers; there is no built-in
IRI denotation or namespace ownership.
[Neo4j: `elementId()`](https://neo4j.com/docs/cypher-manual/current/functions/scalar/#functions-elementid)
[Neo4j: naming and namespace rules](https://neo4j.com/docs/cypher-manual/current/syntax/naming/)

n10s's ordinary triple importer requires a uniqueness constraint on
`:Resource(uri)`, which gives one Neo4j resource node per imported URI. For
vocabulary terms, `handleVocabUris` supports:

- `KEEP`: the full URI becomes the Neo4j schema token;
- `SHORTEN`/`SHORTEN_STRICT`: a stored namespace-prefix map generates shorter
  tokens;
- `MAP`: explicit mappings choose LPG names;
- `IGNORE`: only the local name remains.

`IGNORE` is convenient for Cypher but discards the distinction between, for
example, `https://a.example/status` and `https://b.example/status`. `KEEP`
retains distinction but produces long labels, relationship types, and property
keys that usually require escaped dynamic Cypher. `SHORTEN_STRICT` with a
versioned prefix registry is the least ambiguous compact mapping, but the
prefix registry becomes part of the stored-data contract.
[n10s Graph Config](https://neo4j.com/labs/neosemantics/5.14/reference/#_graph_config_params_global_settings)

### 2. Properties, literal fidelity, and multi-values

Neo4j properties support booleans, strings, integers, floats, temporal/spatial
values, and lists of property values; lists stored as properties cannot contain
`null`. Relationships can also carry properties. This is expressive and
efficient for a product-oriented ABox, but it is not RDF's literal model.
[Neo4j: property values](https://neo4j.com/docs/cypher-manual/current/values-and-types/property-structural-constructed/#property-values)

An RDF literal's identity includes its lexical form, datatype IRI, and, for
language strings, language tag. n10s converts many XSD numeric and temporal
datatypes to Neo4j native values. Its source maps integer-family values to a
Neo4j long and decimal/float/double-family values to a double. Consequently,
distinct RDF literal terms can map to the same Neo4j value—for example lexical
variants of the same integer, or different integer-family datatype IRIs.
[W3C RDF 1.1 Concepts: literal equality](https://www.w3.org/TR/rdf11-concepts/#dfn-literal-term-equality)
[n10s literal mapping source](https://github.com/neo4j-labs/neosemantics/blob/2025.06.1/src/main/java/n10s/RDFToLPGStatementProcessor.java)

n10s defaults are material:

- `handleMultival: OVERWRITE` keeps only the last literal value for a repeated
  predicate; `ARRAY` retains repeated values as a Neo4j array, either globally
  or for a configured predicate allowlist;
- `keepLangTag` defaults to `false`; enabling it encodes the tag with the value
  for n10s helper functions;
- `keepCustomDataTypes` defaults to `false`; enabling it retains selected
  datatype IRIs in an encoded string form, and it is incompatible with some URI
  mapping modes.

Thus “n10s RDF is lossless” is safe only after a supported-RDF-profile and
GraphConfig are fixed and round-trip tested. It must not be inferred from using
an RDF input format alone.
[n10s: multi-valued properties](https://neo4j.com/labs/neosemantics/4.0/import/#_handling_multivalued_properties)
[n10s reference](https://neo4j.com/labs/neosemantics/5.14/reference/#_graph_config_params_global_settings)

Native LPG makes the same choice explicit in the domain. A scalar domain
property can be scalar, a repeatable value can be a list, and a value that
needs independent provenance should be a node/relationship-backed assertion.
The cost is that RDF interoperability becomes a compiler target rather than an
intrinsic representation.

### 3. Named graphs and Asserted/Derived separation

An RDF Dataset contains exactly one default graph and zero or more uniquely
named graphs. Named graphs are the standard structural place to distinguish an
Asserted graph from a Derived graph, although RDF deliberately does not assign
one universal provenance meaning to graph names.
[W3C RDF 1.1 Concepts: RDF datasets](https://www.w3.org/TR/rdf11-concepts/#section-dataset)
[W3C note: semantics of RDF datasets](https://www.w3.org/TR/rdf11-datasets/)

n10s accepts TriG and N-Quads through `n10s.experimental.quadrdf.*`. The
experimental mapper constructs a `ContextResource(uri, graphUri)`, looks up
Neo4j nodes using both fields, and writes `graphUri` on each node. Therefore an
IRI appearing in both Asserted and Derived graphs becomes two physical
`:Resource` nodes. Object relationships are also kept within that context.
[n10s quad procedures](https://github.com/neo4j-labs/neosemantics/blob/2025.06.1/src/main/java/n10s/quadrdf/load/QuadRDFLoadProcedures.java)
[n10s quad mapper](https://github.com/neo4j-labs/neosemantics/blob/2025.06.1/src/main/java/n10s/quadrdf/RDFQuadToLPGStatementProcessor.java)

That mapping has three consequences for Stratum:

1. asking for the complete knowledge about one IRI requires unioning its nodes
   across `graphUri` values;
2. a Derived fact connecting two asserted entities does not connect their
   asserted nodes—it connects their Derived-context copies;
3. ordinary `:Resource(uri)` uniqueness cannot coexist with these duplicates;
   quad import checks for an index instead of the ordinary importer's unique
   constraint.

These are documented/source-observable facts. The architectural inference is
that n10s named graphs are a poor physical fit for an application that wants
one connected entity graph with fact-level origin toggles. They remain useful
when faithful graph-by-graph export is more important than native traversal.

Native LPG has no named graph primitive. A minimal Stratum design can instead
attach `origin = asserted | derived`, `knowledge_base_id`, and materialization
metadata to explicit assertions. If some high-volume binary facts are stored
as direct relationships, those relationships can carry origin fields. Literal
facts that need the same behavior cannot remain ordinary node properties; they
need an assertion node or another first-class record. This is additional domain
design, but it keeps one entity node across both origins.

Using two Neo4j databases for Asserted and Derived data would give coarse
separation but would sacrifice atomic cross-layer updates and ordinary
cross-layer traversal. It should not be the default without a demonstrated
operational isolation requirement.

### 4. Statement-level provenance

Named graphs establish graph membership, not by themselves the complete
evidence chain for one assertion. Fine-grained provenance needs one of:

- a first-class assertion/reifier resource with source, time, producer, rule,
  TBox revision, and evidence links;
- RDF reification/RDF 1.2 reifiers;
- a domain vocabulary that relates a provenance record to the statement; or
- a graph granularity of one provenance unit, which is usually operationally
  excessive.

RDF 1.2 introduces triple terms and reifiers, but on the research date RDF 1.2
Concepts is still a Candidate Recommendation; RDF 1.1 remains the latest W3C
Recommendation. More importantly, n10s `2025.06.1` source implements the older
RDF-star mapping narrowly: an embedded triple used as a subject can become
properties on an object relationship, datatype-property statements are not
mapped that way, and triple terms used as objects are ignored. It is not a
complete RDF 1.2 statement-provenance implementation.
[W3C RDF 1.2 Concepts status](https://www.w3.org/TR/rdf12-concepts/#sotd)
[W3C RDF 1.2: triple terms and reification](https://www.w3.org/TR/rdf12-concepts/#section-triple-terms-reification)
[n10s RDF-star mapping source](https://github.com/neo4j-labs/neosemantics/blob/2025.06.1/src/main/java/n10s/RDFToLPGStatementProcessor.java)

Native LPG relationship properties are convenient for provenance on
object-to-object assertions. They do not solve provenance for a literal stored
as a node property. A uniform evidence chain therefore points toward a narrow
`Assertion` domain object in either option. Once that exists, the decisive
difference is whether its canonical serialization is an LPG pattern or an RDF
assertion vocabulary.

### 5. Schema constraints and SHACL

Neo4j can enforce node/relationship property uniqueness, existence, type, and
keys; several constraint types are Enterprise-only. Its 2026 graph types add
richer source/target and implied-label constraints, also with edition/version
conditions. These constraints protect physical invariants but are not a
replacement for SHACL's graph-oriented validation report.
[Neo4j constraints](https://neo4j.com/docs/cypher-manual/current/schema/constraints/)
[Neo4j graph types](https://neo4j.com/docs/cypher-manual/current/schema/graph-types/set-graph-types/)

n10s can validate a whole graph, a selected node set, or transaction changes.
The transaction mode requires an APOC trigger and rolls back a transaction
that produces validation rows. Its own documentation explicitly says the
implementation covers only part of SHACL.
[n10s SHACL validation](https://neo4j.com/labs/neosemantics/4.0/validation/)

The `2025.06.1` validator source provides a useful implementation allowlist:

- targets: `sh:targetClass`, implicit class targets, and n10s's Cypher-oriented
  `sh:targetQuery` extension;
- paths: direct property/relationship paths and inverse paths;
- constraints: selected datatype/node-kind/class, min/max count, numeric
  bounds, min/max length, pattern, has-value, in, disjoint, closed-shape, and
  limited not patterns.

Unsupported/unproven for Stratum include the remainder of SHACL Core—notably
general logical combinations (`sh:and`, `sh:or`, `sh:xone`), general recursive
shape constraints, the full property-path language, language constraints,
property-pair constraints, and qualified cardinalities—plus SHACL-SPARQL and
SHACL-AF Rules. The safe product contract is an explicit Stratum SHACL profile
with conformance tests, never “SHACL Core” as a blanket claim.
[W3C SHACL Core components](https://www.w3.org/TR/shacl/#core-components)
[n10s validator source](https://github.com/neo4j-labs/neosemantics/blob/2025.06.1/src/main/java/n10s/validation/SHACLValidator.java)

W3C SHACL takes a data graph and shapes graph and produces a validation report;
both input graphs must remain immutable during validation. Inference may be
precomputed or supplied by a supported entailment regime, but is not required.
This reinforces Stratum's accepted boundary: validation and Derived ABox
materialization are different contracts.
[W3C SHACL: validation and graphs](https://www.w3.org/TR/shacl/#validation)
[W3C SHACL: relationship to RDFS inference](https://www.w3.org/TR/shacl/#shacl-rdfs)

### 6. Reasoning and materialization

n10s's inference API expands category, label, and relationship hierarchies at
query time. For example, `nodesLabelled`, `nodesInCategory`, `hasLabel`, and
`getRels` follow explicitly modeled subclass/subcategory/subproperty links and
return explicit plus implicit matches. The documentation describes this as
retrieving information that is not explicitly stored; it does not materialize
an OWL 2 RL closure or an independently lifecycle-managed Derived ABox.
[n10s inferencing](https://neo4j.com/labs/neosemantics/5.14/inference/)

Therefore both alternatives still need a Stratum materializer that defines:

- the supported TBox rule profile;
- deterministic fact identity and deduplication;
- which TBox revision and rule version produced a fact;
- dependency/evidence links;
- invalidation and recomputation;
- convergence and failure recovery.

n10s may be a query helper or import codec, but choosing it does not discharge
this responsibility.

### 7. Cypher ergonomics and API shape

Native LPG can mirror product vocabulary:

```cypher
MATCH (call:ToolCall)-[:INVOKED]->(tool:Tool)
WHERE call.status = 'failed'
RETURN call, tool
```

An RDF-preserving n10s mapping typically adds `:Resource`, a `uri` lookup,
shortened/full-URI schema tokens, array semantics for repeated literals, and,
for quads, `graphUri` filters and duplicate nodes. `KEEP` maximizes namespace
fidelity but makes hand-written Cypher noisy; `IGNORE` maximizes readability
but loses namespace distinction. `MAP` is ergonomic but turns the mapping
registry into a versioned schema migration artifact.

n10s does not provide a general SPARQL endpoint over Neo4j. Its HTTP extension
can describe/export nodes, labels, and Cypher-selected subgraphs as RDF and
“emulates” SPARQL `DESCRIBE`; selection remains by URI/ID/label/property or
Cypher.
[n10s RDF export endpoint](https://neo4j.com/labs/neosemantics/5.14/export/)

For Stratum's strong typed API, neither raw Cypher records nor arbitrary RDF
triples should cross the domain boundary. Both options need typed Rust domain
objects. The difference is whether their invariants are defined in terms of
LPG entities or RDF terms/quads.

### 8. Transactions, constraints, and failure behavior

Neo4j gives both alternatives the same physical ACID transaction base. Its
default isolation is read committed, write locks are acquired automatically,
non-repeatable reads may occur, and deadlocks are detected. Large updates must
be split or otherwise managed because transaction modifications are retained
in memory until completion.
[Neo4j transactional behavior](https://neo4j.com/docs/operations-manual/current/database-internals/)
[Neo4j transaction management](https://neo4j.com/docs/operations-manual/current/database-internals/transaction-management/)

Native LPG lets Stratum select transaction boundaries directly and use
application identity constraints. In n10s ordinary RDF import, `:Resource(uri)`
uniqueness is a prerequisite and provides concurrency safety for resource
identity. GraphConfig is global for the Neo4j graph and, according to the
reference, can be changed or dropped only while the graph is empty. A wrong
namespace/multi-value/literal policy is therefore a reimport migration, not a
small online setting change.
[n10s Graph Config reference](https://neo4j.com/labs/neosemantics/5.14/reference/#_rdf_config)

n10s import defaults to partial commits every 25,000 parsed statements rather
than one transaction for an entire RDF document. The tagged source also offers
a `singleTx` parameter, but a single very large transaction inherits Neo4j's
memory/lock costs. The architectural inference is that a parse or mapping
failure after earlier periodic commits can leave a partial dataset; Stratum
would need an import-run identity, idempotent retry/cleanup, and post-import
validation before promoting data as authoritative.
[n10s parser configuration source](https://github.com/neo4j-labs/neosemantics/blob/2025.06.1/src/main/java/n10s/graphconfig/RDFParserConfig.java)

Other n10s-specific failure modes to design for are:

- accidental multi-value loss under the default `OVERWRITE` mode;
- namespace collision under `IGNORE` or missing-prefix failure under
  `SHORTEN_STRICT`;
- datatype conflict when RDF repeated values map to a Neo4j property array;
- direct Cypher writes that violate the chosen RDF mapping conventions;
- plugin procedure/trigger errors that expose Neo4j or n10s-specific error
  shapes instead of Stratum domain errors;
- partial availability or startup failure when the JAR does not match the
  server line.

### 9. Bulk ingestion

For canonical LPG, Neo4j's native choices include:

- online transactional Cypher and `CALL { ... } IN TRANSACTIONS`;
- `LOAD CSV`;
- `neo4j-admin database import full|incremental`, which writes CSV or Parquet
  into the native store and is intended for millions/billions of entities when
  its operational preconditions are acceptable.

[Neo4j bulk import](https://neo4j.com/docs/operations-manual/current/import/)
[Neo4j `LOAD CSV`](https://neo4j.com/docs/cypher-manual/25/clauses/load-csv/)

n10s adds direct Turtle, N-Triples, JSON-LD, RDF/XML, TriG, and N-Quads parsing
with configurable `commitSize` and `nodeCacheSize`. This avoids building a
separate RDF-to-CSV transform for moderate imports. It is still online
transactional loading into LPG, not the same path as `neo4j-admin`'s native
store writer. Named-graph formats go through the experimental quad importer.
[n10s reference: parser parameters](https://neo4j.com/labs/neosemantics/5.14/reference/#_rdf_import_params)

### 10. Deployment and version compatibility

The decisive documented boundary is deployment: n10s is self-hosted only and
is unavailable on Aura. JSON-LD parsing and the transaction-validation trigger
also introduce APOC dependencies in the documented setup. Native LPG uses only
Neo4j-supported Cypher/driver surfaces and is deployable to Aura.
[n10s availability](https://neo4j.com/labs/neosemantics/#availability-installation)
[n10s installation](https://neo4j.com/labs/neosemantics/5.14/install/)

As of 2026-08-16:

- the latest n10s GitHub release is `2025.06.1`, published 2026-06-16 as a
  security bugfix;
- its `pom.xml` targets Neo4j `2025.06.2` and RDF4J `4.3.12`;
- the repository contains work branches for other Neo4j lines, but an
  unreleased branch is not a support promise or deployable compatibility
  artifact.

[n10s release 2025.06.1](https://github.com/neo4j-labs/neosemantics/releases/tag/2025.06.1)
[n10s build versions](https://github.com/neo4j-labs/neosemantics/blob/2025.06.1/pom.xml)

If n10s is required at runtime, Stratum must pin and test a Neo4j+n10s+APOC
matrix, block unsupported server upgrades, and include plugin startup plus
procedure discovery in readiness. This is a materially larger operational
contract than “Neo4j is the store.”

### 11. Rust integration

Neo4j's official Bolt drivers are .NET, Go, Java, JavaScript, and Python. It
lists Rust `neo4rs` among community drivers and states that community drivers
are not Neo4j-supported products. Neo4j also offers an HTTP Query API for
languages without an official library; its older HTTP API is not available on
Aura.
[Neo4j Bolt drivers](https://neo4j.com/docs/bolt/current/neo4j-drivers/)
[Neo4j community Rust driver](https://neo4j.com/developer/r/)
[Neo4j application APIs](https://neo4j.com/docs/getting-started/languages-guides/)

This is a common risk for both alternatives: Stratum must integration-test
routing, retries, transaction semantics, temporal/list decoding, and Neo4j
version support for the chosen Rust client. n10s adds procedure-specific maps
and rows rather than a Rust-native RDF interface. Rust still sends Cypher such
as `CALL n10s.rdf.import.inline(...)` and decodes dynamically typed procedure
results.

No official primary source establishes an n10s/Rust compatibility contract.
That is a documented gap, not evidence that the integration is impossible.

### 12. Export, portability, and lock-in

RDF's primary benefit is a standard interchange and logical model. n10s can
export RDF from both imported RDF graphs and ordinary property graphs, and can
export the result of a Cypher query. When RDF was imported without namespace
discarding and all relevant value policies preserve the input, the export can
reconstruct the triple view.
[n10s RDF export](https://neo4j.com/labs/neosemantics/5.14/export/)

Portability must still be qualified:

- ordinary triple export does not prove named-graph round-trip;
- mapped Neo4j native types may no longer retain the original RDF literal
  lexical/datatype term;
- relationship properties rely on an RDF-star/reification mapping;
- direct Cypher writes must conform to the n10s mapping conventions;
- the GraphConfig, prefix registry, and explicit mappings are required
  migration artifacts alongside the data.

Native LPG is more directly locked to Neo4j/Cypher at the logical level, but a
narrow Stratum fact algebra plus deterministic RDF exporter can reduce that
lock-in without making n10s the authority. Conversely, a canonical RDF Dataset
lowers logical-model lock-in only if Stratum retains a canonical N-Quads/TriG
representation or validates export round-trips independently of Neo4j. Calling
the n10s-mapped LPG “canonical RDF” without such a proof merely hides the
mapping lock-in.

## Explicit evidence gaps

- Neo4j/n10s publishes no SHACL conformance report mapping the current plugin
  release to every W3C SHACL Core test. The tagged source provides an allowlist,
  not a standards-compliance certificate.
- Neo4j/n10s publishes no guarantee that experimental QuadRDF preserves RDF
  Dataset identity and statement membership for every TriG/N-Quads input. The
  source and tests show its physical strategy; Stratum must supply the
  round-trip proof for its profile.
- Neo4j publishes no official n10s/Rust client compatibility contract. Rust
  integration is ordinary Cypher/procedure invocation through a community
  driver or generic HTTP API.
- No primary source establishes throughput or storage-cost superiority for
  either proposed Stratum shape. Performance claims require Stratum fixtures
  and workload measurements.

## Recommendation options

There is no unconditional winner. The choice depends on which promise Stratum
wants to make now.

### Option A — Native LPG is canonical

Choose this when the next milestone is a working, strongly typed, operational
ABox for Stratum—not general linked-data interoperability.

Required decisions before implementation:

- stable Stratum IDs rather than Neo4j element IDs;
- one entity node across Asserted and Derived facts;
- an explicit assertion representation for every fact that needs evidence,
  including literal values;
- a supported SHACL profile and either a Stratum SHACL-to-Cypher compiler or an
  external validator projection;
- deterministic materialization identity, lineage, invalidation, and replay;
- a versioned RDF import/export mapping if interoperability is needed later.

Benefits: lowest query and deployment friction, Aura remains possible, n10s is
optional, and Rust sees a stable Stratum repository contract. Cost: Stratum
owns the semantic mapping and cannot claim arbitrary RDF Dataset fidelity.

### Option B — RDF Dataset is canonical and n10s is the required Neo4j codec

Choose this only when external RDF interchange, IRI identity, and standard RDF
tool compatibility are current product requirements strong enough to justify a
self-hosted/version-pinned platform.

Minimum guardrails:

- specify an RDF **1.1** profile initially; treat RDF 1.2 features as deferred
  until the Recommendation and n10s implementation converge;
- freeze GraphConfig (`SHORTEN_STRICT` or `KEEP`, `ARRAY`, language/custom
  datatype policy, RDF type mode) as versioned schema;
- maintain round-trip fixtures for every supported term/datatype and reject
  unsupported RDF rather than silently collapsing it;
- do not depend on experimental QuadRDF for authoritative Asserted/Derived
  separation until a prototype proves identity, traversal, deletion, and
  export behavior;
- keep n10s SHACL behind an explicit feature allowlist;
- retain an external canonical N-Quads/TriG snapshot or journal so Neo4j/n10s
  is a reproducible projection, not the only copy of the logical dataset.

Benefits: strongest standards-facing model. Costs: the current named-graph,
literal, SHACL, RDF-star, Aura, and release-matrix limitations all become
product constraints.

### Option C — Narrow Stratum fact model, native LPG backend, RDF as a tested projection

This is the recommended route for the current destination, with one condition:
the team is not committing now to arbitrary third-party RDF ingestion as a
first-class product capability.

Define a small canonical fact algebra that is representable both as LPG and as
an RDF 1.1 Dataset:

```text
EntityId / TermId
AssertionId
PredicateId
Object = EntityRef | TypedValue
Origin = Asserted | Derived(materialization_id)
EvidenceRef*
KnowledgeBaseId
```

Persist it in a native Neo4j LPG optimized for Stratum queries, using
first-class assertions where provenance requires them. Compile the Stratum
TBox into an explicit SHACL profile and validate through a replaceable engine.
Provide deterministic RDF import/export at the boundary and use n10s only as
an optional self-hosted adapter or comparison implementation, not as the
semantic authority.

This option deliberately accepts less RDF expressiveness than Option B. Its
advantage is that it preserves the future migration path: if RDF-native
interoperability later becomes a product requirement, the fact algebra and
round-trip corpus already expose exactly which semantics must be promoted.

## Prototypes that would retire the remaining uncertainty

The architecture decision can be made without benchmarking every path, but the
following bounded prototypes should block a commitment to Option B:

1. Import one TriG fixture where the same IRI occurs in Asserted and Derived
   graphs; query its union, delete only Derived facts, and round-trip to
   canonicalized N-Quads.
2. Round-trip literals covering language tags, custom datatypes, lexical
   variants, temporal offsets, and repeated heterogeneous values under the
   proposed GraphConfig.
3. Run the exact proposed SHACL profile through n10s and a conforming reference
   implementation; diff normalized validation results.
4. Materialize a derived fact with its TBox revision, rule version, and evidence
   chain; invalidate and recompute it without deleting asserted facts.
5. Exercise the chosen Rust driver against the pinned Neo4j server for routing,
   transaction retry, cancellation, list/temporal values, and n10s procedure
   errors.

The pass criteria should be semantic equivalence and recoverability, not only
successful import or visually plausible graph shape.
