# PostgreSQL persistence options for atomic Ontology saves

Research date: 2026-08-07

## Scope

This note investigates PostgreSQL persistence shapes for multiple independent Ontology aggregates. An Ontology is loaded as one graph, edited outside the database, and saved as a complete replacement guarded by an expected revision. The alternatives are:

1. normalized relational tables;
2. one `jsonb` document per Ontology;
3. hybrid shapes.

It records PostgreSQL guarantees and the trade-offs the architecture decision must settle. It does not choose the final shape.

## Findings in brief

- Atomic full replacement does not require a JSON document. PostgreSQL transactions make a multi-statement, multi-table change all-or-nothing and hide its intermediate state from other transactions. A normalized replacement can therefore have the same failure atomicity as a one-row `jsonb` replacement. [PostgreSQL: Transactions](https://www.postgresql.org/docs/current/tutorial-transactions.html)
- A root Ontology row can be the concurrency gate for every shape. A conditional `UPDATE ... WHERE revision = $expected RETURNING revision` is a compare-and-swap (CAS): at the default Read Committed level, a competing updater waits and PostgreSQL re-evaluates the `WHERE` predicate against the committed row version. Only the request whose expected revision still matches can advance it. [PostgreSQL: Transaction Isolation](https://www.postgresql.org/docs/current/transaction-iso.html) [PostgreSQL: `UPDATE`](https://www.postgresql.org/docs/current/sql-update.html)
- Normalization gives PostgreSQL the most opportunity to enforce graph invariants with primary keys, unique constraints, and composite foreign keys. A JSON document gives the application the simplest aggregate-shaped read/write contract, but PostgreSQL cannot express internal object/link references as foreign keys inside a document. [PostgreSQL: Constraints](https://www.postgresql.org/docs/current/ddl-constraints.html)
- `jsonb` supports containment, JSON-path queries, GIN indexes, and targeted expression indexes, but a whole-document load by Ontology ID only needs the root B-tree primary key. Broad JSON indexes should follow demonstrated cross-Ontology query requirements, because they add write and storage overhead. [PostgreSQL: JSON Types](https://www.postgresql.org/docs/current/datatype-json.html) [PostgreSQL: Indexes on Expressions](https://www.postgresql.org/docs/current/indexes-expressional.html)
- Full replacement creates obsolete row versions in every shape. Normalized delete/reinsert creates dead child tuples; changing a document creates a dead root tuple and, for a changed large value, new large-value storage. PostgreSQL relies on vacuum to reclaim obsolete versions. The relative cost is workload- and graph-size-dependent and should be measured rather than inferred from row counts alone. [PostgreSQL: Routine Vacuuming](https://www.postgresql.org/docs/current/routine-vacuuming.html) [PostgreSQL: TOAST](https://www.postgresql.org/docs/current/storage-toast.html)
- “Hybrid” is not a single compromise. A relational identity/reference core with non-duplicated JSON payloads preserves some database invariants. A canonical representation plus a duplicated read/search projection introduces a consistency and repair problem even if both copies are written in one transaction.

## A concurrency protocol shared by all shapes

Use a durable, application-owned `bigint` revision on the Ontology root rather than exposing a PostgreSQL transaction identifier. PostgreSQL transaction IDs are finite and participate in wraparound/freeze maintenance, so they are not an appropriate public aggregate revision. [PostgreSQL: Routine Vacuuming](https://www.postgresql.org/docs/current/routine-vacuuming.html)

The minimal gate is:

```sql
UPDATE ontologies
SET revision = revision + 1,
    updated_at = CURRENT_TIMESTAMP
WHERE ontology_id = $1
  AND revision = $2
RETURNING revision;
```

The statement updates at most one row because `ontology_id` is unique. `RETURNING` reports the post-update revision only for rows actually updated. At Read Committed, two writers with the same expected revision do not both succeed: the second waits for the first row update, then PostgreSQL rechecks its predicate against the new revision. [PostgreSQL: `UPDATE`](https://www.postgresql.org/docs/current/sql-update.html) [PostgreSQL: Transaction Isolation](https://www.postgresql.org/docs/current/transaction-iso.html)

For normalized or projection-bearing storage, execute that CAS **first** in the same transaction, then replace the dependent state, then commit:

```text
BEGIN
  CAS the one Ontology root row
  if no row was returned: ROLLBACK and report a conflict
  replace dependent rows or projections
COMMIT
```

The root update itself acquires the required row lock; a preceding `SELECT FOR UPDATE` is not required merely to serialize saves. Row locks block competing writers/lockers of that root, but do not block ordinary readers. Different Ontologies gate on different root rows, so the protocol does not intentionally serialize all Ontology saves. [PostgreSQL: Explicit Locking](https://www.postgresql.org/docs/current/explicit-locking.html)

There are three API details the database does not choose:

- A zero-row CAS conflates “Ontology does not exist” and “revision is stale.” The API can deliberately expose one conflict outcome, or perform additional locked/read logic to distinguish `404` from `409`.
- The revision increment can overflow. A `bigint` overflow aborts the statement, but the domain must still define the counter's initial value and maximum behavior.
- If the client loses its connection around commit, it may not know whether the transaction committed. The request/response design must define reconciliation (at minimum, re-read the revision and graph; optionally, add an idempotency mechanism). PostgreSQL exposes stable SQLSTATE classes for transaction rollback and unknown transaction resolution; applications should classify codes rather than error text. [PostgreSQL: Error Codes](https://www.postgresql.org/docs/current/errcodes-appendix.html)

### Read consistency

A single `SELECT` sees one statement-level snapshot at Read Committed. A normalized graph assembled by one SQL statement (for example, correlated aggregates returning the root, object types, properties, and links) therefore cannot mix pre-save and post-save child state. If loading uses several `SELECT` statements, Read Committed permits each statement to see a newer snapshot; use one statement or a Repeatable Read transaction when one coherent revision is required across those reads. [PostgreSQL: Transaction Isolation](https://www.postgresql.org/docs/current/transaction-iso.html)

## Option 1: normalized relational tables

### Illustrative shape

The important feature is not the exact table list, but carrying `ontology_id` through keys and references:

```text
ontologies
  primary key (ontology_id), unique(name), revision

object_types
  primary key (ontology_id, object_type_id)
  foreign key (ontology_id) -> ontologies
  unique (ontology_id, api_name)

properties
  primary key (ontology_id, property_id)
  foreign key (ontology_id, object_type_id) -> object_types
  unique (ontology_id, object_type_id, api_name)

link_types
  primary key (ontology_id, link_type_id)
  foreign key (ontology_id, source_object_type_id) -> object_types
  foreign key (ontology_id, target_object_type_id) -> object_types
  unique (ontology_id, api_name)
```

Composite foreign keys that include `ontology_id` make a cross-Ontology property owner or link endpoint unrepresentable. Primary keys and unique constraints create their enforcing B-tree indexes automatically. Foreign keys do **not** automatically index the referencing columns, so indexes beginning with `ontology_id` should be considered for graph loads, scoped deletes, and reference checks. [PostgreSQL: Constraints](https://www.postgresql.org/docs/current/ddl-constraints.html)

Database constraints can directly enforce:

- required scalar fields and scalar checks;
- uniqueness within an Ontology or object type;
- ownership of properties by existing object types;
- existence and same-Ontology scope of link endpoints;
- chosen delete behavior through foreign-key actions.

They cannot by themselves express every semantic graph rule. PostgreSQL explicitly warns that `CHECK` constraints must not depend on other rows and assumes their conditions are immutable; cross-row rules should use `UNIQUE`, `FOREIGN KEY`, or another deliberate mechanism. Rules such as “at least one property has this role” or domain-specific cycles still need aggregate validation in application code or a carefully justified database mechanism. [PostgreSQL: Constraints](https://www.postgresql.org/docs/current/ddl-constraints.html)

### Full-replacement algorithm

After the root CAS succeeds:

1. delete old link rows, property rows, and object-type rows for that `ontology_id` in a fixed dependency order (or use deliberately selected cascades);
2. bulk-insert object types;
3. bulk-insert properties and links;
4. commit.

The inserts should use stable IDs supplied by the aggregate rather than regenerating identity during every save. Inserting referenced object types before properties and links avoids needing deferred foreign keys for this acyclic dependency order. Deferred constraints remain available if a future schema genuinely needs end-of-transaction checking. [PostgreSQL: `CREATE TABLE`](https://www.postgresql.org/docs/current/sql-createtable.html) [PostgreSQL: `SET CONSTRAINTS`](https://www.postgresql.org/docs/current/sql-set-constraints.html)

Any uniqueness, foreign-key, scalar-check, timeout, or database error aborts the transaction; rollback discards the revision increment and all child changes. Other transactions never see the delete/insert gap. [PostgreSQL: Transactions](https://www.postgresql.org/docs/current/tutorial-transactions.html) [PostgreSQL: `ROLLBACK`](https://www.postgresql.org/docs/current/sql-rollback.html)

This simple algorithm maximizes writes. A diff/upsert algorithm can reduce churn for large, lightly changed graphs, but it must also delete stale rows and preserve the same CAS gate. It adds comparison and ordering complexity; it is a performance alternative, not a stronger atomicity mechanism.

For graphs too large for convenient parameterized multi-row inserts, session-local temporary staging tables or `COPY FROM STDIN` can load and validate batches before transferring them into permanent tables. Session-private staging can happen before acquiring the root CAS lock; the CAS and transfer into permanent tables still belong to one transaction. Temporary tables are session-scoped and can be dropped at transaction end. `COPY FROM` invokes target checks and triggers, but a failed large copy can leave invisible dead rows that vacuum must later reclaim. Staging therefore changes throughput and lock duration, not the core CAS guarantee. [PostgreSQL: `CREATE TABLE`](https://www.postgresql.org/docs/current/sql-createtable.html) [PostgreSQL: `COPY`](https://www.postgresql.org/docs/current/sql-copy.html)

### Trade-offs

**Strengths**

- Maximum declarative enforcement of identity, uniqueness, ownership, and link endpoints.
- Direct SQL queries over object types, properties, and links without document expansion.
- Narrow indexes can match known access paths.
- Individual rows are inspectable and repairable with ordinary relational tools.

**Costs**

- One graph load requires joins/aggregates or several reads with explicit snapshot handling.
- Full replacement touches many heap and index rows, generates dead tuples, and increases vacuum/WAL work as graph size and save frequency grow.
- Adding or changing modeled fields commonly requires DDL, backfill, and constraint rollout.
- A generic “property definition” can become awkward if its shape varies greatly by data type; forcing every variant into nullable columns can weaken clarity rather than improve it.

## Option 2: one `jsonb` document per Ontology

### Illustrative shape and save

```text
ontologies
  ontology_id uuid primary key
  name text unique not null
  revision bigint not null
  schema_document jsonb not null
  updated_at timestamptz not null
```

The CAS can replace `schema_document` in the same `UPDATE` that advances `revision`, so the common save needs one statement. The common load is one primary-key lookup and already has the aggregate shape expected by the API.

PostgreSQL generally recommends `jsonb` over `json` for processing and indexing. `jsonb` validates JSON syntax, stores a decomposed representation, discards insignificant whitespace/key order, and collapses duplicate object keys to the last value. The API must not assign semantics to JSON object order or duplicate keys. [PostgreSQL: JSON Types](https://www.postgresql.org/docs/current/datatype-json.html)

### Constraint boundary

PostgreSQL can constrain root relational columns and can use same-row `CHECK` expressions for small, immutable document predicates. It does not provide native JSON Schema enforcement, and internal IDs in a JSON array cannot be targets of ordinary foreign keys. Consequently, duplicate IDs, dangling link endpoints, type-dependent property rules, and other graph invariants are primarily application validation unless the design adds custom immutable functions, triggers, or a relational projection.

Custom constraint functions carry migration risk: PostgreSQL assumes a `CHECK` condition is immutable and does not automatically revalidate stored rows when a function's behavior changes. The documented safe sequence is to drop, change, and re-add the constraint so existing rows are checked again. [PostgreSQL: Constraints](https://www.postgresql.org/docs/current/ddl-constraints.html)

### Index and update behavior

- The B-tree primary key on `ontology_id` serves whole-aggregate load/save.
- GIN can support containment, key-existence, and JSON-path searches across documents. The default `jsonb_ops` and `jsonb_path_ops` operator classes have different query and size/selectivity trade-offs.
- A targeted expression index can be smaller and faster for a stable path than a broad GIN index, but expression indexes are recomputed on inserts and non-HOT updates.
- If the product does not query inside documents across Ontologies, a JSON GIN index has no demonstrated job.

[PostgreSQL: JSON Types](https://www.postgresql.org/docs/current/datatype-json.html) [PostgreSQL: Indexes on Expressions](https://www.postgresql.org/docs/current/indexes-expressional.html)

Any update locks the whole containing row, which aligns with one-writer-per-Ontology aggregate semantics but prevents independent concurrent edits inside the same document. PostgreSQL stores large values transparently with TOAST, which can compress and/or move them out of line. An unchanged out-of-line value can be preserved across an unrelated row update, but replacing the document changes that value and should be treated as a whole-aggregate write for capacity testing. [PostgreSQL: JSON Types](https://www.postgresql.org/docs/current/datatype-json.html) [PostgreSQL: TOAST](https://www.postgresql.org/docs/current/storage-toast.html)

### Trade-offs

**Strengths**

- Persistence shape, API payload, and editor aggregate align directly.
- Read and replace are simple, with one row lock and one conditional update.
- Many additive document fields do not require table DDL.
- Unknown or variant-specific metadata can be retained without a wide sparse schema.

**Costs**

- Most graph invariants are only as strong as the application validator and the exclusivity of the write path.
- SQL analytics and targeted repair inside nested arrays are more complex.
- Every save changes the large value even for a small logical edit; lock time, WAL, TOAST churn, and vacuum behavior must be tested at expected graph sizes.
- Flexible storage does not remove data-format migrations. Old documents still require explicit recognition, validation, and backfill when semantics change.

## Option 3: hybrid shapes

### Relational core plus non-duplicated JSON payloads

Stable identities and graph references can remain normalized while variant metadata is stored once in `jsonb`, for example:

```text
object_types (ontology_id, object_type_id, api_name, metadata jsonb)
properties   (ontology_id, property_id, object_type_id, api_name,
              value_type, constraints jsonb)
link_types   (ontology_id, link_type_id, source_object_type_id,
              target_object_type_id, api_name, metadata jsonb)
```

This retains relational uniqueness and foreign keys for the graph skeleton without requiring a column for every type-specific option. It still uses the normalized transaction/replacement algorithm and still needs application validation for rules inside JSON payloads.

The key boundary decision is which fields are stable, queryable invariants and which are opaque/variant payload. Moving a field across that boundary later is a data migration.

### Canonical form plus duplicated projection

Another hybrid stores the complete canonical document and duplicates selected nodes/fields into relational search tables or a cached document assembled from normalized rows. A transaction can update both physical representations atomically, but built-in row constraints cannot generally prove that an arbitrary JSON graph and a set of relational rows are semantically identical. Generated columns cannot solve a cross-row projection because their expressions cannot use subqueries or reference other rows. [PostgreSQL: Generated Columns](https://www.postgresql.org/docs/current/ddl-generated-columns.html)

This design therefore needs all of the following to be explicit:

- exactly one source of truth;
- whether projections are synchronously updated or asynchronously rebuilt;
- behavior when projection maintenance fails;
- a deterministic rebuild/repair operation;
- whether reads may tolerate a stale projection;
- migrations for both representations.

Atomic dual writes prevent externally visible half-commits, but they do not prevent an application bug from committing two mutually inconsistent values. This hybrid pays for itself only when a demonstrated read/search path cannot be served adequately by the canonical representation.

## Locking, failure, and operational implications

### Locks and deadlocks

`INSERT`, `UPDATE`, and `DELETE` take row-exclusive table locks, which do not globally serialize ordinary data modifications. The root row is the intended per-Ontology serialization point. Avoid `TRUNCATE` for aggregate replacement: it is table-wide and takes an access-exclusive lock. [PostgreSQL: Explicit Locking](https://www.postgresql.org/docs/current/explicit-locking.html)

If a future operation saves multiple Ontologies in one transaction, acquire root locks in a deterministic order. PostgreSQL detects deadlocks by aborting one transaction; applications should retry the **entire** transaction for `deadlock_detected` (`40P01`) and, when using Serializable, `serialization_failure` (`40001`). [PostgreSQL: Explicit Locking](https://www.postgresql.org/docs/current/explicit-locking.html) [PostgreSQL: Error Codes](https://www.postgresql.org/docs/current/errcodes-appendix.html)

### Failure atomicity checklist

For every shape, integration tests should establish that:

1. a stale expected revision changes neither revision nor graph;
2. validation or constraint failure after the CAS rolls back the revision;
3. failure midway through normalized/projection insertion leaves the previous graph intact;
4. two concurrent saves with the same expected revision yield exactly one commit;
5. saves to different Ontologies can proceed independently;
6. a reader never receives a graph assembled from two revisions;
7. a retry after an uncertain connection outcome reconciles against the committed revision/content.

### Replacement churn

PostgreSQL MVCC retains obsolete versions from updates and deletes until vacuum can reclaim them. This affects both JSONB and normalization, with different granularity. Observe save latency, rows/bytes written, WAL volume, dead tuples, table plus TOAST size, index size, and autovacuum behavior under realistic graph distributions. [PostgreSQL: Routine Vacuuming](https://www.postgresql.org/docs/current/routine-vacuuming.html) [PostgreSQL: TOAST](https://www.postgresql.org/docs/current/storage-toast.html)

## Migration implications

### Normalized

- New first-class fields generally require `ALTER TABLE`; changed semantics may require backfill and constraints.
- A non-volatile constant default can be added without rewriting every existing row, while volatile defaults can require updating each row.
- Foreign-key and check constraints can be introduced `NOT VALID`, enforced for new writes, then validated against old rows with a lower-strength lock.
- Production indexes can be built with `CREATE INDEX CONCURRENTLY`, which permits writes but does more work, cannot run inside a transaction block, and can leave an invalid index after failure.

[PostgreSQL: `ALTER TABLE`](https://www.postgresql.org/docs/current/sql-altertable.html) [PostgreSQL: `CREATE INDEX`](https://www.postgresql.org/docs/current/sql-createindex.html)

### JSONB

- Adding an optional field may need no database DDL, but readers and validators still need defined behavior for documents written by older software.
- Required fields, renamed fields, or changed semantics need an explicit document backfill or a supported multi-shape read period.
- Updating documents during backfill creates new row versions and may rewrite large values.
- New GIN/expression indexes have the same live-migration concerns as relational indexes.
- A storage-format discriminator can make compatibility fail-closed and migrations observable; whether to include one is a separate choice from retaining Ontology history.

### Hybrid

- A relational-core/JSON-payload model needs migrations whenever a field crosses the invariant/payload boundary.
- A duplicated projection needs coordinated schema and data migration for both forms plus a way to rebuild and verify the projection.
- The migration order must preserve the declared source of truth throughout mixed-version deployment.

## Trade-offs the architecture decision must explicitly resolve

1. **Authority:** Is the aggregate document authoritative, are normalized rows authoritative, or are stable relational fields plus JSON payloads jointly authoritative without duplication?
2. **Invariant placement:** Which MVP rules must remain true even if a writer bypasses the HTTP validator? Normalization is materially different if same-Ontology endpoints and uniqueness must be database-enforced.
3. **Actual access paths:** Is the only hot read “load one complete Ontology by ID,” or must the system search/filter object types, properties, and links across many Ontologies?
4. **Graph scale and save rate:** What are the expected p50/p95/max serialized bytes, node/property/link counts, and saves per Ontology? These numbers determine whether document rewrite or multi-row replacement is the dominant cost.
5. **Read assembly:** Will a normalized load be one SQL snapshot or a Repeatable Read transaction? The contract must not accidentally assemble a graph through unrelated Read Committed snapshots.
6. **Replacement strategy:** Is simple delete/reinsert sufficient, or does measured churn justify diff/upsert or staging? The latter are optimizations that add failure paths.
7. **Conflict semantics:** Does a failed CAS return one generic conflict, or must the API reliably distinguish missing from stale? How does it reconcile an unknown commit outcome?
8. **Schema evolution:** Is frequent, heterogeneous metadata expected enough to justify JSON payloads? How will stored shapes be identified, rejected, and backfilled?
9. **Projection tolerance:** If a hybrid duplicates data, is projection lag allowed, and what repairs it? If no demonstrated query needs duplication, what benefit offsets that consistency burden?
10. **Operational budget:** What limits are imposed on graph size and transaction duration, and what telemetry will detect lock waits, WAL amplification, dead tuples, and vacuum lag?

## Primary sources

- [PostgreSQL: Transactions](https://www.postgresql.org/docs/current/tutorial-transactions.html)
- [PostgreSQL: Transaction Isolation](https://www.postgresql.org/docs/current/transaction-iso.html)
- [PostgreSQL: Explicit Locking](https://www.postgresql.org/docs/current/explicit-locking.html)
- [PostgreSQL: `UPDATE`](https://www.postgresql.org/docs/current/sql-update.html)
- [PostgreSQL: `ROLLBACK`](https://www.postgresql.org/docs/current/sql-rollback.html)
- [PostgreSQL: Constraints](https://www.postgresql.org/docs/current/ddl-constraints.html)
- [PostgreSQL: `CREATE TABLE`](https://www.postgresql.org/docs/current/sql-createtable.html)
- [PostgreSQL: `SET CONSTRAINTS`](https://www.postgresql.org/docs/current/sql-set-constraints.html)
- [PostgreSQL: JSON Types](https://www.postgresql.org/docs/current/datatype-json.html)
- [PostgreSQL: Generated Columns](https://www.postgresql.org/docs/current/ddl-generated-columns.html)
- [PostgreSQL: Indexes on Expressions](https://www.postgresql.org/docs/current/indexes-expressional.html)
- [PostgreSQL: `CREATE INDEX`](https://www.postgresql.org/docs/current/sql-createindex.html)
- [PostgreSQL: `ALTER TABLE`](https://www.postgresql.org/docs/current/sql-altertable.html)
- [PostgreSQL: `COPY`](https://www.postgresql.org/docs/current/sql-copy.html)
- [PostgreSQL: Routine Vacuuming](https://www.postgresql.org/docs/current/routine-vacuuming.html)
- [PostgreSQL: TOAST](https://www.postgresql.org/docs/current/storage-toast.html)
- [PostgreSQL: Error Codes](https://www.postgresql.org/docs/current/errcodes-appendix.html)
