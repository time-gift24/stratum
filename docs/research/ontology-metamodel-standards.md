# Ontology metamodel standards and constraint semantics

Research date: 2026-08-10

## Decision

Stratum Ontology should be specified as a **closed application metamodel**, not
as an RDF Schema or OWL reasoning ontology and not as a SHACL implementation.
Its MVP schema entities are Ontology, Object Type, Property, and Link Type.
They have explicit typed identities, ownership, references, and validation
rules. The persisted schema contains no object instances and produces no
logical entailments.

The closest formal metamodel precedent is OMG MOF/EMOF: a class owns typed
properties, a binary association has typed ends and multiplicities, identifiers
are independent of mutable model data, and lifecycle/versioning services are
separated from the structural model. The closest validation precedent is
SHACL: it separates shapes from data, counts values explicitly, checks value
types, can close the allowed property set, and returns structured validation
results. Stratum should borrow those concepts without adopting either syntax,
runtime, interchange format, or conformance claim.

RDFS and OWL remain useful negative controls. Their domain, range, subclass,
inverse, and cardinality constructs describe logical consequences under
open-world semantics. They do not mean “reject this finite document because a
field is absent or an endpoint has the wrong declared type.” In particular,
OWL may infer missing endpoint types, and a functional property with two named
values may imply that the two names denote the same individual. Those outcomes
conflict with Stratum's stable-identity and validation semantics.

This yields the following immediate model boundary:

- Object Type, Property, and Link Type remain separate first-class entities.
- A Property is exclusively owned by one Object Type, but ownership does not
  mean embedding it as anonymous JSON. It retains its own immutable
  `PropertyId` and, in the accepted normalized persistence direction, its own
  canonical row carrying the owner ID.
- A Link Type is a separate binary association definition, not a Property and
  not a pair of forward/reverse properties.
- `required` and Link Type cardinalities are declarations about a future object
  data plane. The MVP validates that the declarations are well formed; it does
  not yet validate object instances because none are stored.
- Canvas layout remains editor metadata and has no schema or inference effect.

## Evidence scope and specification status

The conclusions use stable primary standards:

- [RDF Schema 1.1](https://www.w3.org/TR/rdf-schema/) (W3C
  Recommendation);
- [OWL 2 Structural Specification and Functional-Style Syntax, Second
  Edition](https://www.w3.org/TR/owl2-syntax/) and the
  [OWL 2 Primer](https://www.w3.org/TR/owl2-primer/) (W3C
  Recommendations);
- [Shapes Constraint Language (SHACL)](https://www.w3.org/TR/shacl/) (W3C
  Recommendation, 2017);
- [OMG Meta Object Facility Core 2.5.1](https://www.omg.org/spec/MOF/2.5.1/PDF)
  (formal OMG specification).

As of the research date, [SHACL 1.2 Core](https://www.w3.org/TR/shacl12-core/)
is a W3C Working Draft dated 3 August 2026. Its own status section says that
Working Draft publication does not imply W3C endorsement and points to the 2017
SHACL Recommendation as the latest Recommendation. This report therefore uses
the stable Recommendation for normative comparisons. The 1.2 family also puts
rule-based inference in a separate draft specification, which reinforces the
need to keep validation and inference distinct; no 1.2 feature is proposed for
the MVP.

## Keep three semantic layers separate

“Ontology” can obscure whether a statement describes the schema language, a
user-defined schema, or user data. The MVP should preserve this separation:

| Layer | Stratum artifact | What exists in the MVP | What is checked |
| --- | --- | --- | --- |
| Product metamodel | Rust/domain definitions for Ontology, Object Type, Property, Link Type, IDs, and cardinality enums | Yes | Whether a submitted Candidate schema is a legal instance of the Stratum metamodel |
| User schema model | One persisted Ontology and its Object Types, owned Properties, Link Types, and layout | Yes | Identity, ownership, scoped names, scalar types, endpoint references, limits, and layout invariants |
| Object data plane | Objects classified by Object Types, scalar values, and Link instances | No | Nothing in this delivery; Property requiredness and Link maxima are declarations only |

This is a practical separation, not a claim that Stratum implements MOF's
architecture. MOF itself warns against treating metamodeling as a rigid
four-layer stack: the essential relationship is between classifier and
instance, and reflection can cross any number of layers
([MOF 2.5.1, section 7.3](https://www.omg.org/spec/MOF/2.5.1/PDF)). Stratum only
needs the three concrete roles above.

The separation also rules out OWL-style metamodeling in the MVP. OWL 2 permits
one IRI to be used as a class and as an individual (“punning”), with the two
views interpreted independently under Direct Semantics
([OWL 2 metamodeling](https://www.w3.org/TR/owl2-syntax/#Metamodeling)). An
Object Type ID in Stratum has exactly one entity kind and cannot simultaneously
be an object instance, Property, or Link Type ID.

## What each standard actually models

| System | Primary role | Property model | Meaning of constraints | Fit for Stratum |
| --- | --- | --- | --- | --- |
| RDF Schema | Vocabulary for RDF resources | Properties are global resources described by domain and range; classes do not own field declarations | Domain, range, subclass, and subproperty statements contribute entailments | Vocabulary inspiration only; wrong ownership and validation semantics |
| OWL 2 | Logic-based knowledge representation | Separate object, data, and annotation properties; class expressions can restrict them | Open-world axioms support consistency checking and inference | Explicitly not the MVP runtime or validator |
| SHACL | Description and validation of RDF graph shapes | Property shapes select values through property paths and apply constraints | Finite graph validation produces conformance and validation results; shapes may be open or closed | Best validation precedent, but Stratum has its own DTOs and validator |
| MOF/EMOF | Formal definition of metamodels and model instances | Classes own typed Properties; Associations have member ends; multiplicity is explicit | Model/instance structural conformance and OCL constraints | Best entity-boundary and ownership precedent, without a conformance goal |

RDFS explicitly contrasts its property-centric model with object-oriented
class-owned attributes: it describes a property by the classes to which it
applies, allowing new properties to be added without redefining the class
([RDFS introduction](https://www.w3.org/TR/rdf-schema/#ch_introduction)). That
is valuable for an extensible Web vocabulary, but it is the opposite of the
confirmed Stratum rule that a Property is owned by exactly one Object Type.

EMOF is much closer structurally. Its merged model includes Class,
Class-owned `ownedAttribute` Properties, typed elements, explicit lower/upper
multiplicity, binary Associations, member ends, generalization, and opposites
([MOF 2.5.1, sections 12.2-12.4](https://www.omg.org/spec/MOF/2.5.1/PDF)).
EMOF restricts an Association to exactly two member ends, and each member end
must be typed by a Class. These are useful precedents for explicit binary Link
Types, though Stratum deliberately uses a smaller and differently named model.

## Inference is not validation

### RDFS domain and range infer classifications

For a property `P`, RDFS `domain C` states that subjects using `P` are
instances of `C`; `range C` states that objects reached through `P` are
instances of `C`. Multiple domains or ranges mean membership in all listed
classes
([RDFS domain](https://www.w3.org/TR/rdf-schema/#ch_domain),
[RDFS range](https://www.w3.org/TR/rdf-schema/#ch_range)). The specification
does not prescribe that an application reject a graph missing an explicit type
triple; it notes that applications may use this information for checking,
editing assistance, or reasoning.

Therefore, an RDFS domain or range is not equivalent to a Stratum Link Type
endpoint foreign key. A Stratum candidate must already contain both referenced
Object Type definitions. The validator either resolves those exact typed IDs
inside the same Ontology or reports a dangling endpoint. It never creates or
infers an Object Type.

### OWL is deliberately open world

The OWL 2 Primer says OWL is not a syntax-conformance schema language and
cannot require that a piece of information be syntactically present. It also
contrasts a database's usual closed-world assumption with OWL's open-world
assumption: a fact absent from an OWL document may simply be unknown rather
than false
([OWL 2 Primer, “What is OWL 2?”](https://www.w3.org/TR/owl2-primer/#What_is_OWL_2.3F)).

OWL's structural specification makes the consequence concrete. An object
property domain axiom infers that the subject is in the domain class, and a
range axiom infers that the object is in the range class. It explicitly warns
that these differ from database or object-system checks
([OWL 2 object-property domain and range](https://www.w3.org/TR/owl2-syntax/#Object_Property_Domain)).

OWL cardinality is also not Stratum cardinality validation. A
`FunctionalObjectProperty` says that each individual has at most one distinct
value. If two differently named values are asserted, OWL can entail that they
are the same individual because OWL does not make the unique-name assumption;
it need not report a two-value validation error
([OWL 2 functional object properties](https://www.w3.org/TR/owl2-syntax/#Functional_Object_Properties)).
Stratum IDs, by contrast, are deliberately distinct typed identities. A future
instance validator must count distinct IDs and reject an excess; it must never
repair a cardinality violation by equating identities.

OWL's minimum, maximum, and exact cardinalities are class expressions: they
describe the individuals connected to at least, at most, or exactly a number
of distinct individuals. Combined with open-world semantics, an absent
property assertion is not proof that the minimum is violated
([OWL 2 object-property cardinality restrictions](https://www.w3.org/TR/owl2-syntax/#Object_Property_Cardinality_Restrictions)).
They therefore cannot implement Stratum Property requiredness. A future
Stratum instance validator would inspect the complete instance representation
and report absence directly, following count-validation semantics instead.

### SHACL supplies validation semantics, with a qualifier

SHACL separates a shapes graph from a data graph and validates selected focus
nodes. Property shapes can constrain a path's value nodes by datatype, class,
and count. `sh:minCount` yields a validation result when too few values are
present; `sh:maxCount` does so when too many are present
([SHACL cardinality constraint components](https://www.w3.org/TR/shacl/#core-components-count)).
`sh:datatype` checks literal datatype rather than inferring one
([SHACL datatype constraint](https://www.w3.org/TR/shacl/#DatatypeConstraintComponent)).

This is the right semantic family for Stratum's future object-instance
validation, but it is not automatically “fully closed world.” A SHACL node
shape only rejects properties not enumerated by its property shapes when
`sh:closed` is true; `sh:ignoredProperties` can add exceptions
([SHACL closed constraint](https://www.w3.org/TR/shacl/#ClosedConstraintComponent)).
SHACL processors may also support an entailment regime
([SHACL entailment](https://www.w3.org/TR/shacl/#shacl-rdfs)). Thus, the exact
validation data graph and inference configuration remain part of a validator's
contract.

For Stratum MVP, “closed” has a narrower, already confirmed meaning: the
Candidate schema is the complete desired **metadata aggregate**. Unknown DTO
fields, duplicate entities, dangling IDs, or missing owned children are
evaluated against that finite candidate. This does not yet decide whether a
future Object instance may carry fields that are absent from its Object Type.
Instance property closure is a later data-plane decision and must not be
silently inferred from whole-schema replacement.

## Concept-by-concept mapping

### Identity and names

MOF defines an identifier in the context of an extent and motivates immutable
identity specifically so model elements can be correlated while user data such
as names change
([MOF 2.5.1, section 10](https://www.omg.org/spec/MOF/2.5.1/PDF)). It also
separates model structure from identity, lifecycle, versioning, queries, and
other services as an explicit design goal
([MOF 2.5.1, section 7.2](https://www.omg.org/spec/MOF/2.5.1/PDF)).

Stratum should retain that principle, using its already selected typed UUIDv7
IDs rather than MOF Extents/URIs or RDF/OWL IRIs:

- the ID is immutable and references use it;
- `name`, `display_name`, and `description` are mutable metadata;
- changing a name does not change identity;
- a deleted identity is not reused;
- an existing Property ID cannot move to another Object Type, and an ID cannot
  change entity kind.

RDF and OWL use IRIs as Web-scale names/identifiers and support cross-document
vocabularies and imports. Those are not current Stratum requirements. Adding an
IRI, namespace, alias, `sameAs`, blank-node identity, or provider-compatible ID
would introduce another identity system without a consumer and is rejected for
the MVP.

### Object Type

An Object Type is an explicit schema entity within exactly one Ontology. It
defines the owner scope for Properties and may be the source or target of Link
Types. It is closest to a small EMOF Class or a class-targeted SHACL node shape,
but it is neither an RDFS/OWL class resource nor executable reasoner input.

The MVP has no abstract Object Types, metaclasses, unions, intersections,
disjointness, equivalent classes, keys, class expressions, or inferred class
membership. Those constructs would change instance classification and schema
validation semantics and are not harmless metadata additions.

### Property ownership and scalar range

RDFS properties are global and domain declarations do not make a property an
owned component of a class. OWL data properties are also independent entities;
class restrictions may reuse them in many class expressions. SHACL property
shapes may be named and reused, and their paths can be inverse or multi-step.
None of those models establishes Stratum's exclusive field ownership.

EMOF's Class-owned typed Property is the closest precedent. Stratum specializes
it further:

- every Property is a first-class entity with `PropertyId`;
- exactly one Object Type owns it for its entire lifetime;
- its `name` is unique only inside that owner;
- its range is exactly one of the six confirmed scalar value types;
- it is not a Link Type, association end, reusable Property Type, arbitrary
  path, collection, nested shape, or generic JSON definition.

This supports a separate normalized Property relation with both
`ontology_id` and `object_type_id` references. Semantic composition controls
ownership and deletion; it does not require denormalized storage inside an
Object Type document. Exact SQL names and indexes belong to the persistence
decision, not to these external standards.

The RDFS/OWL word “range” should not appear in the Stratum API for scalar
Properties. In those languages it participates in inference. Stratum's
`value_type` is a closed validation enum. SHACL's direct datatype check is a
better model for what a future instance validator would do.

### Property requiredness

The combination of SHACL count constraints and MOF multiplicity gives a clear
translation for scalar Properties:

| Stratum Property | Conceptual lower bound | Conceptual upper bound |
| --- | ---: | ---: |
| `required: false` | 0 | 1 |
| `required: true` | 1 | 1 |

Optional means absence, not a `null` value. The upper bound is always one
because all MVP Properties are scalar. This translation is explanatory; the
wire contract remains the explicit `required` boolean and `value_type` enum.
The MVP must not import generic lower/upper integers, defaults, derived values,
ordered/unique collections, or qualified cardinalities merely because MOF,
OWL, or SHACL can express them.

### Link Type direction, traversal, and cardinality

A Link Type is one separately identified binary association with a canonical
source Object Type and target Object Type. The source/target choice gives the
relation its authored direction and endpoint roles. Querying or displaying the
same relation from the target is reverse traversal of that one relation.

Three tempting equivalences are false:

1. **Reverse traversal is not an OWL inverse axiom.** OWL
   `InverseObjectProperties(P Q)` relates two property expressions and entails
   assertions using the inverse property
   ([OWL 2 inverse object properties](https://www.w3.org/TR/owl2-syntax/#Inverse_Object_Properties)).
   Stratum has no second property `Q` and materializes no inferred assertion.
2. **Reverse traversal is not symmetry.** A parent relation can be traversed
   from child to parent without asserting that the child is also parent of the
   parent. OWL symmetric-property semantics would make that inference, so it is
   rejected.
3. **A Link Type is not a scalar Property whose value happens to be an ID.**
   That would collapse association identity, reverse cardinality, and graph
   traversal into a field and conflict with the confirmed separate entity.

EMOF's binary Association and opposite-end model is the closest structural
analogy. MOF also notes that a lightweight reverse-navigation role can avoid
the storage and referential-integrity burden of a second opposite Property
([MOF 2.5.1, section 12.6](https://www.omg.org/spec/MOF/2.5.1/PDF)). Stratum
arrives at an even smaller representation: one Link Type row plus two endpoint
references, with traversal supported in either direction.

The directional maximums translate as follows:

| Stratum field value | Minimum | Maximum |
| --- | ---: | ---: |
| `one` | 0 | 1 |
| `many` | 0 | unbounded |

`source_to_target` counts targets per source; `target_to_source` counts sources
per target. These are future instance-conformance rules, not OWL functional or
inverse-functional axioms. Required links, exact counts, arbitrary bounds,
transitivity, symmetry, reflexivity, property chains, and derived links remain
out of scope.

### Inheritance and interfaces

RDFS `subClassOf` is transitive and entails that every instance of a subclass
is also an instance of its superclass
([RDFS subclass](https://www.w3.org/TR/rdf-schema/#ch_subclassof)). OWL extends
that model with rich class expressions and restrictions. MOF generalization
causes inherited structural features to participate in instances and model
conformance
([MOF 2.5.1, sections 12 and 15](https://www.omg.org/spec/MOF/2.5.1/PDF)).

Adding even a simple `parent_object_type_id` would therefore require decisions
about inherited Property identity, name conflicts, requiredness, Link endpoint
substitutability, cycles, multiple inheritance, deletion, and migration of
future instances. “Interface” adds similar conformance and conflict rules. The
confirmed MVP excludes both, so it must not add parent IDs, `implements`,
abstract flags, inherited-property projections, or subclass expansion to
neighborhood queries. These are deferred as a coherent future capability, not
reserved fields.

### Annotations and editor metadata

RDFS provides `rdfs:label` for a human-readable name and `rdfs:comment` for a
description
([RDFS label](https://www.w3.org/TR/rdf-schema/#ch_label),
[RDFS comment](https://www.w3.org/TR/rdf-schema/#ch_comment)). OWL annotations
can attach nonlogical information to entities and axioms, while OWL
metamodeling is reserved for information intended to affect the modeled domain
([OWL 2 metamodeling and annotations](https://www.w3.org/TR/owl2-syntax/#Metamodeling)).
MOF offers generic string name/value Tags and explicitly assigns no meaning to
their values
([MOF 2.5.1, section 11](https://www.omg.org/spec/MOF/2.5.1/PDF)).

Stratum should keep only the explicit fields it already needs:

- `name` for a scoped programmatic handle;
- `display_name` for human-readable presentation;
- `description` for human-readable explanation;
- canvas positions in a separate layout component.

It should not add a generic annotations/tags map, annotation properties,
localized label graph, provenance, or custom metadata escape hatch in the MVP.
Those mechanisms weaken the closed contract and have no current consumer.
Canvas order and coordinates are presentation metadata and must not affect
schema equality, cardinality, traversal, or future object conformance.

### Deletion behavior

RDFS and OWL define graph and logical semantics, not Stratum's aggregate CRUD
lifecycle. Under open-world semantics, removing an axiom does not assert its
negation, and the same consequence may still follow from other axioms. SHACL
reports whether data conforms to shapes; it does not define resource history or
hard deletion.

MOF's useful distinction is composition versus reference. Its instance model
requires at most one composition owner and forbids cyclic containment, and its
delete semantics removes composite children and slots
([MOF 2.5.1, sections 12.5 and 15](https://www.omg.org/spec/MOF/2.5.1/PDF)).
MOF also treats lifecycle and versioning as services orthogonal to structural
modeling rather than automatically embedding them in every metamodel.

Stratum should keep its already confirmed, smaller rule:

- Ontology owns Object Types, Link Types, and layout metadata;
- Object Type compositionally owns Properties;
- Link Type references two Object Types but is not owned by either endpoint;
- omission from a successfully saved complete Candidate hard-deletes a prior
  child;
- removing an Object Type requires every incident Link Type and position to be
  absent from that same candidate;
- a failed candidate changes nothing;
- no tombstone, deprecation state, history, rollback, or logical negation is
  implied.

This is persistence deletion, not ontology axiom retraction. The distinction
should be explicit in domain documentation and tests.

## Validation model for the MVP

The metadata validator answers one question: “Is this complete candidate a
valid instance of the Stratum Ontology metamodel?” It does not ask an OWL
reasoner whether the candidate is logically satisfiable, and it does not run a
SHACL engine over object data.

At minimum, the validator should preserve the confirmed checks:

1. every entity has a valid typed ID and an existing ID has not changed kind or
   Property owner;
2. names satisfy their grammar and scoped uniqueness rules;
3. every Property has exactly one owner in the same Ontology, a recognized
   scalar value type, and explicit requiredness;
4. every Link Type has exactly one source and target reference resolving in the
   same candidate plus two recognized maximum-cardinality values;
5. layout positions reference present Object Types and remain semantically
   inert;
6. limits and all cross-entity invariants are evaluated over the complete
   candidate;
7. every deterministic violation is returned, sorted by path and code, and any
   violation produces zero writes.

SHACL provides the precedent for a structured report: its validation report
contains conformance plus validation results, and conforming processors must be
capable of returning all required results
([SHACL validation report](https://www.w3.org/TR/shacl/#validation-report)).
Stratum should retain its own JSON Pointer paths, stable machine codes, and safe
messages rather than serializing SHACL RDF results.

The database remains a final invariant boundary for identities, ownership, and
references. In a normalized model, Object Type, Property, Link Type, and canvas
position are distinct canonical relations; composite keys or equivalent
constraints should carry `ontology_id` so cross-Ontology ownership and
endpoints are unrepresentable. Application validation is still necessary to
produce the complete, path-addressed report and enforce whole-candidate rules.
This report settles the semantic entity boundaries, not exact table names,
indexes, or transaction sequencing.

## Adopt, defer, reject

### Adopt now

- explicit, typed schema-entity identity independent of mutable names;
- Object Type as a first-class definition and Property as a separately
  identified, exclusively owned child entity;
- Link Type as a separately identified binary association with explicit source
  and target references;
- scalar Property type plus lower-bound requiredness and fixed upper bound one;
- two explicit maximum-cardinality directions for Link Types, with all minima
  fixed at zero;
- strict complete-candidate validation and a deterministic multi-result report;
- explicit human metadata fields separated from schema semantics;
- composition-aware hard deletion and reference-aware endpoint validation.

### Defer behind a new data-plane or schema-language decision

- validation and persistence of object instances;
- SHACL import/export or a SHACL-compatible profile;
- inheritance, interfaces, abstract types, and shared Property Types;
- required links, arbitrary numeric bounds, collections, structured values,
  enums, defaults, and derived values;
- localized annotations, generic extension metadata, provenance, and imports;
- relation characteristics such as symmetry, transitivity, reflexivity, keys,
  property chains, or calculated inverses;
- lifecycle/versioning, draft/publish, migration of object instances, and
  deprecation.

### Explicitly reject for the MVP

- RDFS/OWL open-world inference as the meaning of endpoint or value-type
  declarations;
- silently inferring missing schema entities, Properties, endpoint types, or
  reverse Link Types;
- OWL identity merging, `sameAs`, a lack of unique names, or class/individual
  punning;
- treating bidirectional traversal as a symmetric relation;
- treating a complete Candidate schema as equivalent to SHACL `sh:closed` on
  future object instances;
- using RDF/OWL IRIs, blank nodes, external provider IDs, generic tags, or
  arbitrary JSON as second identity or extension systems;
- claiming RDF Schema, OWL, SHACL, MOF, or Palantir compatibility.

## Planning consequences

1. Keep the API vocabulary `Object Type`, `Property`, and `Link Type`; do not
   rename them to Class, Slot, ObjectProperty, Shape, or Association in public
   contracts.
2. Keep separate domain types and normalized canonical persistence for Object
   Types, Properties, and Link Types. `Property` ownership is encoded by a
   typed owner reference, not by erasing Property identity into a blob.
3. Implement one purpose-built deterministic candidate validator. Do not add an
   RDF store, OWL reasoner, SHACL engine, MOF runtime, or semantic-Web crate for
   the MVP.
4. Document explicitly that Link neighborhood traversal follows stored Link
   Types in both directions but performs no inference and creates no inverse or
   symmetric relation.
5. Document explicitly that Property requiredness and Link maxima are not
   currently enforced against objects because object instances are outside the
   bounded context.
6. If the data plane is later introduced, open a separate decision covering
   instance closure, conformance timing, migration, inherited schemas, and
   whether a restricted SHACL export is useful. Do not reserve speculative MVP
   fields for it now.

## Primary sources

- [W3C RDF Schema 1.1](https://www.w3.org/TR/rdf-schema/)
- [W3C OWL 2 Structural Specification and Functional-Style Syntax, Second
  Edition](https://www.w3.org/TR/owl2-syntax/)
- [W3C OWL 2 Primer, Second Edition](https://www.w3.org/TR/owl2-primer/)
- [W3C Shapes Constraint Language (SHACL), 2017 Recommendation](https://www.w3.org/TR/shacl/)
- [W3C SHACL 1.2 Core Working Draft status](https://www.w3.org/TR/shacl12-core/)
- [OMG Meta Object Facility Core Specification 2.5.1](https://www.omg.org/spec/MOF/2.5.1/PDF)
