# Open-source ontology metadata implementations and storage models

Research date: 2026-08-10

## Scope

This note compares two implementations whose official documentation exposes materially different metamodels: LinkML and TypeDB. It asks what their choices teach a PostgreSQL-backed Stratum Ontology service; it does not seek compatibility with either system.

The confirmed Stratum MVP is narrower than both implementations:

- an Ontology contains Object Types, Object-Type-owned Properties, binary Link Types, and canvas positions;
- object instances, inference, inheritance, shared Properties, n-ary relations, version workflows, and physical data-source bindings are out of scope;
- the HTTP boundary reads and conditionally replaces a complete Ontology, while PostgreSQL uses normalized canonical tables;
- a read-only endpoint returns an N-hop Object Type neighborhood.

Only official documentation is used below. Observed facts are separated from recommendations.

## LinkML

### Observed metamodel

LinkML describes a schema as an instance of the LinkML metamodel. Its model documentation represents the schema as a YAML or JSON document with sections such as `classes` and `slots`. Slots are first-class definitions independent of classes and may be reused by multiple classes. A class lists global slots that it permits. LinkML models are closed by default: data containing a slot not listed for the class is invalid. [LinkML models](https://linkml.io/linkml/schemas/models.html)

LinkML also permits a class to define `attributes` inline. The official documentation describes this as a convenience alternative to a separate reusable slot definition and explicitly notes that inline attributes are harder to reuse outside the class hierarchy. [LinkML models: attributes](https://linkml.io/linkml/schemas/models.html#the-attributes-slot)

The LinkML metamodel exposes all of the following references from `ClassDefinition` to `SlotDefinition`:

- `slots` for use of a named slot;
- `slot_usage` for class-context refinements;
- `attributes` for a definition written in the class;
- `defining_slots` and slot conditions for richer modeling.

It also exposes `SchemaDefinition.slot_definitions` as the indexed collection of slot definitions. [LinkML `SlotDefinition`](https://linkml.io/linkml-model/latest/docs/SlotDefinition/)

A slot can use another class as its range and can be multivalued, so the same slot mechanism can represent scalar fields and object-valued references. Constraints such as range and value bounds live on slot definitions or their contextual usage. [LinkML models](https://linkml.io/linkml/schemas/models.html)

### Persistence boundary

LinkML's authored source is one metamodel document, commonly YAML or JSON. Generators can derive other schema forms from it. This makes the document the authoring source and gives reuse and inheritance first-class syntax; it does not imply that an application Ontology backed by PostgreSQL should copy its physical storage shape.

### What Stratum should and should not copy

The reusable global-slot plus contextual-override model solves a real problem, but it is the problem Stratum calls Shared Property Type and has deliberately deferred. Introducing a global slot row and an Object-Type-to-slot binding now would turn every local Property into an indirect binding before reuse exists.

For the MVP, Stratum should instead keep:

- one `Property` entity with its own immutable ID;
- exactly one owning Object Type;
- its scalar value type, required declaration, names, and description on that Property;
- no global Property definition and no override layer.

If Shared Property Type later enters scope, it should arrive as a new definition-and-binding model with explicit migration semantics rather than being disguised inside today's local Property.

## TypeDB

### Observed metamodel

TypeDB implements a Polymorphic Entity-Relation-Attribute model. A schema defines entity types, relation types, attribute types, and their interactions through interfaces. Entity types are standalone; relation types declare scoped role types; attribute types declare a primitive value type. [TypeDB entities, relations, and attributes](https://typedb.com/docs/core-concepts/typeql/entities-relations-attributes/)

Entity and relation types may implement an attribute's ownership interface with `owns`. Multiple types may own the same attribute type. They may implement relation-role interfaces with `plays`, while a relation declares roles with `relates`. Role labels are scoped by their relation type. [TypeDB entities, relations, and attributes](https://typedb.com/docs/core-concepts/typeql/entities-relations-attributes/)

Consequently, a TypeDB relationship is not merely a source and target. A relation type can declare one or more roles, and different entity or relation types can play those roles. Relations themselves may own attributes or play roles in other relations. [TypeDB data and query model](https://typedb.com/docs/typeql-reference/data-model/)

TypeDB attributes are globally named types rather than fields exclusively declared inside one entity. Ownership carries constraints: for example, `@key` on an ownership combines uniqueness with exactly-one cardinality. [TypeDB `@key`](https://typedb.com/docs/typeql-reference/annotations/key/)

TypeDB schema definitions are stored and manipulated inside a TypeDB database using TypeQL `define`, `redefine`, and `undefine` operations. The schema constrains permitted data and participates in query validation and optimization. [TypeQL reference](https://typedb.com/docs/typeql-reference/) [Create a TypeDB schema](https://typedb.com/docs/home/get-started/schema)

### Persistence boundary

TypeDB is itself the schema-and-instance database. Its type system and storage engine enforce ownership, role, inheritance, and instance constraints together. Its official documentation does not justify reproducing that internal graph representation as PostgreSQL tables in Stratum.

### What Stratum should and should not copy

TypeDB demonstrates the value of making relationship and attribute definitions explicit, queryable schema entities. It also demonstrates how quickly the model expands when reusable attributes, roles, n-ary relations, inheritance, and instance enforcement are required.

The Stratum MVP should therefore retain explicit rows but not TypeDB semantics:

- `ontology_properties` rows are directly owned by one Object Type, not globally shared attribute types connected by ownership edges;
- `ontology_link_types` rows have one canonical source and target, not arbitrary role vertices;
- the two directional maximum cardinalities stay on the Link Type;
- bidirectional traversal does not synthesize inverse relationships or role types;
- `required` and cardinality remain metadata declarations until object instances exist.

## Comparative result

| Concern | LinkML | TypeDB | Stratum MVP |
| --- | --- | --- | --- |
| Authored schema source | YAML/JSON metamodel document | Schema inside a TypeDB database | Normalized PostgreSQL rows |
| Property definition | Reusable global slot or inline attribute | Independent attribute type plus ownership | Independent row owned by one Object Type |
| Relationship | Class-valued slot or richer class model | First-class, role-based, potentially n-ary relation | Binary Link Type with source and target |
| Constraint context | Slot definition and class usage | Type, ownership, role, and instance enforcement | Aggregate validation plus relational integrity |
| Reuse/inheritance | First-class | First-class | Deferred |
| Instance data | Validated/generated against schema | Stored and enforced in same database | Out of scope |

The apparent similarity of names hides different semantics. LinkML's slot and TypeDB's attribute are reusable definitions; Stratum's current Property is an Object-Type-owned field. TypeDB's relation is role-based and potentially n-ary; Stratum's Link Type is deliberately binary.

## PostgreSQL recommendation for Stratum

The complete-document HTTP API is an aggregate and transaction boundary, not a physical storage instruction. Use one canonical normalized representation:

- `ontologies` for aggregate identity, mutable names, revision, and timestamps;
- `ontology_object_types` for Object Types;
- `ontology_properties` with a same-Ontology foreign key to its owning Object Type;
- `ontology_link_types` with same-Ontology foreign keys to source and target Object Types;
- `ontology_canvas_positions` keyed by Object Type;
- ordinal columns where the HTTP contract promises stable array order.

Do not store a second canonical JSON document or a duplicated query projection. PostgreSQL constraints should enforce identities, scoped name uniqueness, ownership, endpoints, enum domains, and delete dependencies. Rust aggregate validation should enforce count limits, complete deterministic validation reporting, and rules that span the candidate graph.

The N-hop endpoint benefits directly from separate Link Type rows and indexes beginning with `(ontology_id, source_object_type_id)` and `(ontology_id, target_object_type_id)`. A recursive query can discover the Object Type set; one consistent read transaction can then assemble Object Types, Properties, induced Link Types, and positions.

## Decision boundary

Adopt the explicitness of these implementations, not their richer semantics:

- **Adopt now:** first-class IDs; separate Property and Link Type rows; closed aggregate validation; explicit value types; explicit relationship endpoints and multiplicities.
- **Defer:** Shared Property Type, binding overrides, inheritance/interfaces, n-ary roles, instance keys, instance validation, and schema generation.
- **Reject for the MVP:** treating global slots, attribute ownership edges, or role vertices as hidden implementation machinery for today's simpler model.

This boundary leaves future features addable as explicit new concepts without making the current schema pretend those concepts already exist.

## Primary sources

- [LinkML models](https://linkml.io/linkml/schemas/models.html)
- [LinkML `SlotDefinition` metamodel](https://linkml.io/linkml-model/latest/docs/SlotDefinition/)
- [TypeDB entities, relations, and attributes](https://typedb.com/docs/core-concepts/typeql/entities-relations-attributes/)
- [TypeDB data and query model](https://typedb.com/docs/typeql-reference/data-model/)
- [TypeDB TypeQL reference](https://typedb.com/docs/typeql-reference/)
- [TypeDB `@key` annotation](https://typedb.com/docs/typeql-reference/annotations/key/)
- [TypeDB schema tutorial](https://typedb.com/docs/home/get-started/schema)
