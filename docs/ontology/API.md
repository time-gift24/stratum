# Ontology HTTP API Contract

This document fixes the MVP HTTP contract for the Ontology bounded context. During implementation, utoipa-generated OpenAPI is the protocol authority; this document is the design input and frontend handoff.

## Conventions

- Every route uses the `/v1` prefix and JSON unless the response has no body.
- All request DTOs reject unknown fields.
- `OntologyId`, `ObjectTypeId`, `PropertyId`, and `LinkTypeId` are UUIDv7 strings. They are distinct typed identities even though their JSON representation is a string.
- The service creates an Ontology ID. The client creates Object Type, Property, and Link Type IDs before the first save; the service validates and preserves them without remapping.
- `name` matches `^[a-z][a-z0-9_]{0,63}$`. `display_name` is required Unicode text with 1–200 characters. An optional `description`, when present, contains 1–2,000 characters; use omission rather than `null`.
- JSON arrays preserve client order on a successful save. Their order is presentation metadata, not Ontology schema semantics.
- Timestamps use RFC 3339 UTC strings.
- No endpoint in this contract stores object instances.

## Resource document

An individual Ontology resource has this shape:

```json
{
  "id": "0198f5e8-92ce-7c52-b55f-ecdc06090f4a",
  "name": "support_domain",
  "display_name": "Support domain",
  "description": "Schema used by support agents",
  "object_types": [
    {
      "id": "0198f5e9-2eca-7b7c-93d7-b3ba92976384",
      "name": "customer",
      "display_name": "Customer",
      "description": "A customer account",
      "properties": [
        {
          "id": "0198f5e9-8d1b-721d-b0d5-c68c3c00a0f5",
          "name": "email",
          "display_name": "Email",
          "description": "Primary contact address",
          "value_type": "string",
          "required": true
        }
      ]
    }
  ],
  "link_types": [
    {
      "id": "0198f5ea-0475-76df-a4fc-745f0b76c69d",
      "name": "owns_ticket",
      "display_name": "Owns ticket",
      "description": "Relates a customer to a ticket",
      "source_object_type_id": "0198f5e9-2eca-7b7c-93d7-b3ba92976384",
      "target_object_type_id": "0198f5ea-a471-75f2-934f-ddc646eb7736",
      "source_to_target": "many",
      "target_to_source": "one"
    }
  ],
  "canvas": {
    "positions": [
      {
        "object_type_id": "0198f5e9-2eca-7b7c-93d7-b3ba92976384",
        "x": 120.5,
        "y": -48.0
      }
    ]
  }
}
```

Required rules:

- `object_types`, every `properties`, `link_types`, `canvas`, and `canvas.positions` are required, including when empty.
- `description` is optional on Ontology, Object Type, Property, and Link Type. Other shown fields are required.
- `value_type` is one of `string`, `integer`, `number`, `boolean`, `date`, or `date_time`.
- Each cardinality is `one` or `many`; `one` means zero or one and `many` means zero or more.
- Every Link Type endpoint and canvas position references an Object Type in the same document.
- At most one canvas position exists for an Object Type. Coordinates are finite JSON numbers.
- Object Type names and Link Type names have separate Ontology-local namespaces. Property names are unique within their owning Object Type.
- Object Type, Property, and Link Type IDs are each globally unique across all currently existing Ontologies within their typed ID namespace, not only within one document. A Candidate whose child entity ID already belongs to another live Ontology is rejected with `409 ontology_entity_id_conflict`, not a Candidate-internal `422` violation, and neither Ontology changes.
- Deletion is hard: removed entities leave no tombstone. The client is responsible for never intentionally reusing a deleted ID (the editor always generates fresh UUIDv7 IDs for new entities); because MVP has no identity registry, the service does not promise to detect a deliberately resubmitted historical ID.
- An empty Ontology and an Object Type without properties are valid.

The document is the complete desired state. A successful replacement permanently deletes prior child entities omitted from it. Removing an Object Type also requires removing every incident Link Type and its canvas position.

## Routes

### List Ontologies

```http
GET /v1/ontologies?page=1&per_page=20&sort=-updated_at
```

- `page` defaults to 1.
- `per_page` defaults to 20 and accepts 1–100.
- `sort` defaults to `-updated_at`. Supported fields are `name`, `display_name`, `created_at`, and `updated_at`; prefix with `-` for descending order.
- Equal sort values are ordered by `id` ascending.
- `search` is optional and matches `name` and `display_name` with a case-insensitive substring (contains) match. Leading and trailing whitespace is ignored, a blank value disables filtering, and a value longer than 100 characters is rejected with `400`. The term is matched literally: `%`, `_`, and `\` carry no wildcard meaning.

`200 OK`:

```json
{
  "data": [
    {
      "id": "0198f5e8-92ce-7c52-b55f-ecdc06090f4a",
      "name": "support_domain",
      "display_name": "Support domain",
      "description": "Schema used by support agents",
      "created_at": "2026-08-08T08:30:00Z",
      "updated_at": "2026-08-08T09:15:00Z"
    }
  ],
  "pagination": {
    "page": 1,
    "per_page": 20,
    "total": 1
  }
}
```

Pages beyond the result set return an empty `data` array and the real `total`.

### Create an Ontology

```http
POST /v1/ontologies
Content-Type: application/json
```

```json
{
  "name": "support_domain",
  "display_name": "Support domain",
  "description": "Schema used by support agents"
}
```

The service creates an Ontology containing empty `object_types`, `link_types`, and `canvas.positions` collections.

`201 Created` returns the complete resource document and these headers:

```http
Location: /v1/ontologies/{ontology_id}
ETag: "opaque-strong-validator"
```

### Read an Ontology

```http
GET /v1/ontologies/{ontology_id}
```

`200 OK` returns the complete resource document and its strong `ETag` header.

### Replace an Ontology atomically

```http
PUT /v1/ontologies/{ontology_id}
If-Match: "etag-returned-by-the-last-resource-read-or-save"
Content-Type: application/json
```

The body is the complete resource document. The path and body Ontology IDs must match. `If-Match` must contain exactly one strong ETag previously emitted for that resource.

The service validates the complete candidate before writing. Either schema and canvas layout are both saved, or neither is saved. On success it stores the submitted JSON representation without injecting defaults, changing values, or reordering arrays.

`204 No Content` has no response body and returns the new strong `ETag` header.

The ETag is an opaque transport validator backed by an internal monotonically increasing revision. Revision is not a JSON field and does not provide readable history.

### Delete an Ontology

```http
DELETE /v1/ontologies/{ontology_id}
If-Match: "etag-returned-by-the-last-resource-read-or-save"
```

`If-Match` follows the same rule as replacement. `204 No Content` permanently removes the Ontology aggregate.

### Read an Object Type neighborhood

```http
GET /v1/ontologies/{ontology_id}/object-types/{object_type_id}/neighborhood?depth=2
```

- `depth` defaults to 1 and accepts integers from 0 through 5.
- Traversal starts at the path `object_type_id` and follows Link Types in both directions.
- The result contains every Object Type reachable in at most `depth` hops.
- It contains the induced Link Type subgraph: every saved Link Type whose two endpoints are both in the returned Object Type set.
- Each returned Object Type includes all of its properties.
- Canvas positions are included for returned Object Types when present.
- Arrays retain their relative order from the saved Ontology document.
- This is a read-only projection of the persisted Ontology. It is not a Candidate schema, has no ETag, and cannot be sent to the replacement endpoint.

`200 OK`:

```json
{
  "origin_object_type_id": "0198f5e9-2eca-7b7c-93d7-b3ba92976384",
  "depth": 2,
  "object_types": [],
  "link_types": [],
  "canvas": {
    "positions": []
  }
}
```

`object_types`, `link_types`, and `canvas` use exactly the corresponding shapes from the resource document. At depth 0, the result contains only the origin Object Type, its properties, its optional position, and any self-Link Types on the origin.

The editable canvas computes neighborhoods locally from its in-memory Candidate schema so unsaved edits are visible. This endpoint provides the same focus model for the canonical persisted graph and non-editor consumers.

## Validation errors

An invalid but well-formed Candidate schema returns `422 Unprocessable Content` with every deterministic violation found:

```json
{
  "error": {
    "code": "invalid_ontology_schema",
    "message": "ontology schema is invalid",
    "violations": [
      {
        "code": "duplicate_object_type_name",
        "path": "/object_types/1/name",
        "message": "object type name is already used in this ontology"
      }
    ]
  }
}
```

- `code` is stable and machine-readable.
- `path` is an RFC 6901 JSON Pointer into the submitted document.
- `message` is safe human-readable text and is not a frontend control-flow contract.
- Violations are sorted by `path`, then `code`.
- A validation failure performs zero writes and does not change the ETag.

All other errors use:

```json
{
  "error": {
    "code": "ontology_not_found",
    "message": "ontology was not found"
  }
}
```

## Status and error matrix

| Status | Stable code | Meaning |
| --- | --- | --- |
| 400 | `invalid_request` | Malformed JSON, unknown fields, malformed IDs or headers, invalid query values, or path/body ID mismatch |
| 404 | `ontology_not_found` | The path Ontology does not exist |
| 404 | `object_type_not_found` | The neighborhood origin does not exist in the path Ontology |
| 409 | `ontology_name_conflict` | Create or replacement would violate deployment-wide Ontology name uniqueness |
| 409 | `ontology_entity_id_conflict` | A submitted Object Type, Property, or Link Type ID already belongs to another live Ontology |
| 412 | `ontology_precondition_failed` | The supplied ETag is no longer current |
| 413 | `ontology_payload_too_large` | The Ontology request exceeds the route body limit |
| 422 | `invalid_ontology_schema` | A well-formed complete candidate violates schema or canvas invariants |
| 428 | `ontology_precondition_required` | `If-Match` is absent on PUT or DELETE |
| 500 | `internal_error` | An unexpected internal failure occurred |
| 503 | `ontology_store_unavailable` | The persistence dependency is unavailable |

On `412`, the client keeps its local Candidate schema, reads the latest resource and ETag, and asks the user to reconcile. It must not silently retry the stale candidate with a newer ETag.

## MVP limits

- POST and PUT request bodies: 2 MiB maximum on these routes; other API routes retain their own limits.
- Object Types per Ontology: 500.
- Properties per Object Type: 100; total Properties per Ontology: 10,000.
- Link Types per Ontology: 2,000.
- Canvas positions per Ontology: 500.
- Neighborhood depth: 0–5.

Count and text-limit violations in a syntactically valid document are included in the `422` validation report. The byte body limit is enforced before JSON decoding and returns `413`.

## Frontend save state

The editor keeps three logical values:

- `acknowledged`: the last resource document and ETag confirmed by the server;
- `candidate`: the mutable local document shown by the canvas;
- `in_flight`: the immutable document and base ETag of the current PUT attempt.

Edits made while a request is in flight continue in `candidate`. A successful PUT only acknowledges the exact `in_flight` document and returned ETag; if `candidate` has moved on, the next save uses that new ETag. A failed validation maps violations by JSON Pointer and leaves `candidate` intact. A `412` also leaves it intact and requires reconciliation.

The frontend may persist a crash-recovery draft in IndexedDB as `{ ontology_id, base_etag, candidate }`. After a timeout or lost response, it reads the resource before retrying to determine whether the in-flight representation committed. Real-time presence, operation logs, OT/CRDT merge, and automatic offline merge are outside the MVP.

## OpenAPI requirements

- Every handler is included in utoipa-generated OpenAPI under an `Ontology` tag.
- Every request, success response, error envelope, violation, pagination value, enum, and identifier DTO derives or otherwise provides `ToSchema`.
- Every documented status includes its response body type; `204` responses document the relevant ETag header and no body.
- Enum values, formats, minimums, maximums, regexes, and body limits are represented where OpenAPI supports them.
- The generated specification, not a separately maintained protocol document, becomes authoritative once implementation starts.
