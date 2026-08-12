//! Container-backed verification for the canonical Ontology PostgreSQL store.
//!
//! Run with `make -C crates/stratum-ontology test-integration`.

use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

use sqlx::PgPool;
use stratum_ontology::{
    Canvas, CanvasPosition, Cardinality, CreateOntology, LinkType, LinkTypeId, ListOntologies,
    ListSort, ObjectType, ObjectTypeId, Ontology, OntologyError, OntologyId, OntologyStore,
    OntologyStoreError, Property, PropertyId, ValueType,
};
use tokio::sync::Mutex;

const DEFAULT_DATABASE_URL: &str =
    "postgres://stratum_ontology:stratum_ontology@127.0.0.1:54329/stratum_ontology_test";
const MAX_OBJECT_TYPES: usize = 500;
const MAX_PROPERTIES_PER_OBJECT_TYPE: usize = 20;
const MAX_LINK_TYPES: usize = 2_000;
const MEASUREMENTS: usize = 20;

static INTEGRATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn integration_lock() -> &'static Mutex<()> {
    INTEGRATION_LOCK.get_or_init(|| Mutex::new(()))
}

fn database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned())
}

async fn connect_store() -> OntologyStore {
    let database_url = database_url();
    OntologyStore::connect(&database_url)
        .await
        .expect("PostgreSQL container should be available")
}

async fn connect_pool() -> PgPool {
    let database_url = database_url();
    PgPool::connect(&database_url)
        .await
        .expect("PostgreSQL container should be available")
}

fn unique_name(prefix: &str) -> String {
    format!("{prefix}_{}", OntologyId::new().as_uuid().simple())
}

fn create_input(prefix: &str) -> CreateOntology {
    CreateOntology {
        name: unique_name(prefix),
        display_name: format!("{prefix} ontology"),
        description: Some("container integration fixture".to_owned()),
    }
}

fn one_object_document(
    root: &Ontology,
    object_type_id: ObjectTypeId,
    property_id: PropertyId,
) -> Ontology {
    let mut document = root.clone();
    document.display_name = "One object document".to_owned();
    document.object_types = vec![ObjectType {
        id: object_type_id,
        name: "thing".to_owned(),
        display_name: "Thing".to_owned(),
        description: Some("One stored object type".to_owned()),
        properties: vec![Property {
            id: property_id,
            name: "name".to_owned(),
            display_name: "Name".to_owned(),
            description: None,
            value_type: ValueType::String,
            required: true,
        }],
    }];
    document.link_types = Vec::new();
    document.canvas = Canvas {
        positions: vec![CanvasPosition {
            object_type_id,
            x: 4.0,
            y: 8.0,
        }],
    };
    document
}

fn linked_document(
    root: &Ontology,
    source_id: ObjectTypeId,
    target_id: ObjectTypeId,
    link_id: LinkTypeId,
) -> Ontology {
    let mut document = root.clone();
    document.display_name = "Linked document".to_owned();
    document.object_types = vec![
        ObjectType {
            id: source_id,
            name: "source".to_owned(),
            display_name: "Source".to_owned(),
            description: None,
            properties: Vec::new(),
        },
        ObjectType {
            id: target_id,
            name: "target".to_owned(),
            display_name: "Target".to_owned(),
            description: None,
            properties: Vec::new(),
        },
    ];
    document.link_types = vec![LinkType {
        id: link_id,
        name: "connects".to_owned(),
        display_name: "Connects".to_owned(),
        description: None,
        source_object_type_id: source_id,
        target_object_type_id: target_id,
        source_to_target: Cardinality::Many,
        target_to_source: Cardinality::One,
    }];
    document.canvas = Canvas {
        positions: Vec::new(),
    };
    document
}

fn ordered_document(root: &Ontology) -> Ontology {
    let person_id = ObjectTypeId::new();
    let company_id = ObjectTypeId::new();
    let mut document = root.clone();
    document.display_name = "Ordered persisted document".to_owned();
    document.object_types = vec![
        ObjectType {
            id: person_id,
            name: "person".to_owned(),
            display_name: "Person".to_owned(),
            description: Some("A person".to_owned()),
            properties: vec![
                Property {
                    id: PropertyId::new(),
                    name: "email".to_owned(),
                    display_name: "Email".to_owned(),
                    description: Some("Email address".to_owned()),
                    value_type: ValueType::String,
                    required: true,
                },
                Property {
                    id: PropertyId::new(),
                    name: "name".to_owned(),
                    display_name: "Name".to_owned(),
                    description: None,
                    value_type: ValueType::String,
                    required: true,
                },
            ],
        },
        ObjectType {
            id: company_id,
            name: "company".to_owned(),
            display_name: "Company".to_owned(),
            description: None,
            properties: vec![Property {
                id: PropertyId::new(),
                name: "name".to_owned(),
                display_name: "Name".to_owned(),
                description: None,
                value_type: ValueType::String,
                required: false,
            }],
        },
    ];
    document.link_types = vec![
        LinkType {
            id: LinkTypeId::new(),
            name: "employs".to_owned(),
            display_name: "Employs".to_owned(),
            description: Some("Company employs person".to_owned()),
            source_object_type_id: company_id,
            target_object_type_id: person_id,
            source_to_target: Cardinality::Many,
            target_to_source: Cardinality::One,
        },
        LinkType {
            id: LinkTypeId::new(),
            name: "knows".to_owned(),
            display_name: "Knows".to_owned(),
            description: None,
            source_object_type_id: person_id,
            target_object_type_id: person_id,
            source_to_target: Cardinality::Many,
            target_to_source: Cardinality::Many,
        },
    ];
    document.canvas = Canvas {
        positions: vec![
            CanvasPosition {
                object_type_id: company_id,
                x: 32.0,
                y: 16.0,
            },
            CanvasPosition {
                object_type_id: person_id,
                x: 8.0,
                y: 24.0,
            },
        ],
    };
    document
}

struct TraversalFixture {
    document: Ontology,
    alpha: ObjectTypeId,
    bravo: ObjectTypeId,
    charlie: ObjectTypeId,
    delta: ObjectTypeId,
    self_link: LinkTypeId,
    reverse_link: LinkTypeId,
    alpha_to_charlie: LinkTypeId,
    induced_link: LinkTypeId,
    all_links: Vec<LinkTypeId>,
}

fn traversal_fixture(root: &Ontology) -> TraversalFixture {
    let alpha = ObjectTypeId::new();
    let bravo = ObjectTypeId::new();
    let charlie = ObjectTypeId::new();
    let delta = ObjectTypeId::new();
    let object_type = |id: ObjectTypeId, name: &str| ObjectType {
        id,
        name: name.to_owned(),
        display_name: format!("{name} type"),
        description: None,
        properties: vec![Property {
            id: PropertyId::new(),
            name: "label".to_owned(),
            display_name: "Label".to_owned(),
            description: None,
            value_type: ValueType::String,
            required: false,
        }],
    };
    let link =
        |name: &str, source_object_type_id: ObjectTypeId, target_object_type_id: ObjectTypeId| {
            LinkType {
                id: LinkTypeId::new(),
                name: name.to_owned(),
                display_name: format!("{name} link"),
                description: None,
                source_object_type_id,
                target_object_type_id,
                source_to_target: Cardinality::Many,
                target_to_source: Cardinality::Many,
            }
        };

    let self_link = link("self_alpha", alpha, alpha);
    let reverse_link = link("bravo_to_alpha", bravo, alpha);
    let alpha_to_charlie = link("alpha_to_charlie", alpha, charlie);
    let induced_link = link("bravo_to_charlie", bravo, charlie);
    let charlie_to_delta = link("charlie_to_delta", charlie, delta);
    let delta_to_bravo = link("delta_to_bravo", delta, bravo);
    let all_links = vec![
        self_link.id,
        reverse_link.id,
        alpha_to_charlie.id,
        induced_link.id,
        charlie_to_delta.id,
        delta_to_bravo.id,
    ];

    let mut document = root.clone();
    document.display_name = "Traversal graph".to_owned();
    document.object_types = vec![
        object_type(charlie, "charlie"),
        object_type(alpha, "alpha"),
        object_type(delta, "delta"),
        object_type(bravo, "bravo"),
    ];
    document.link_types = vec![
        self_link,
        reverse_link,
        alpha_to_charlie,
        induced_link,
        charlie_to_delta,
        delta_to_bravo,
    ];
    document.canvas = Canvas {
        positions: vec![
            CanvasPosition {
                object_type_id: bravo,
                x: 1.0,
                y: 1.0,
            },
            CanvasPosition {
                object_type_id: alpha,
                x: 2.0,
                y: 2.0,
            },
            CanvasPosition {
                object_type_id: delta,
                x: 3.0,
                y: 3.0,
            },
        ],
    };
    TraversalFixture {
        document,
        alpha,
        bravo,
        charlie,
        delta,
        self_link: all_links[0],
        reverse_link: all_links[1],
        alpha_to_charlie: all_links[2],
        induced_link: all_links[3],
        all_links,
    }
}

fn maximum_document(root: &Ontology) -> Ontology {
    let mut object_types = Vec::with_capacity(MAX_OBJECT_TYPES);
    for object_index in 0..MAX_OBJECT_TYPES {
        let mut properties = Vec::with_capacity(MAX_PROPERTIES_PER_OBJECT_TYPE);
        for property_index in 0..MAX_PROPERTIES_PER_OBJECT_TYPE {
            properties.push(Property {
                id: PropertyId::new(),
                name: format!("property_{property_index}"),
                display_name: format!("Property {property_index}"),
                description: None,
                value_type: ValueType::String,
                required: false,
            });
        }
        object_types.push(ObjectType {
            id: ObjectTypeId::new(),
            name: format!("type_{object_index}"),
            display_name: format!("Type {object_index}"),
            description: None,
            properties,
        });
    }
    let object_ids = object_types
        .iter()
        .map(|object_type| object_type.id)
        .collect::<Vec<_>>();
    let mut link_types = Vec::with_capacity(MAX_LINK_TYPES);
    for link_index in 0..MAX_LINK_TYPES {
        let (source_object_type_id, target_object_type_id) = if link_index < MAX_OBJECT_TYPES - 1 {
            let target_index = link_index + 1;
            let source_index = target_index.saturating_sub(100);
            (object_ids[source_index], object_ids[target_index])
        } else {
            // Exercise every expansion step with the surplus links while
            // keeping them inside one layer so none can shorten the five hops.
            let surplus_index = link_index - (MAX_OBJECT_TYPES - 1);
            let layer_start = 1 + (surplus_index % 4) * 100;
            let layer_offset = (surplus_index / 4) % 100;
            let source_index = layer_start + layer_offset;
            let target_index = layer_start + ((layer_offset + 1) % 100);
            (object_ids[source_index], object_ids[target_index])
        };
        link_types.push(LinkType {
            id: LinkTypeId::new(),
            name: format!("edge_{link_index}"),
            display_name: format!("Edge {link_index}"),
            description: None,
            source_object_type_id,
            target_object_type_id,
            source_to_target: Cardinality::Many,
            target_to_source: Cardinality::Many,
        });
    }
    let positions = object_ids
        .iter()
        .copied()
        .map(|object_type_id| CanvasPosition {
            object_type_id,
            x: 0.0,
            y: 0.0,
        })
        .collect();

    let mut document = root.clone();
    document.display_name = "Maximum fixture".to_owned();
    document.object_types = object_types;
    document.link_types = link_types;
    document.canvas = Canvas { positions };
    document
}

fn p95(samples: &mut [Duration]) -> Duration {
    assert!(!samples.is_empty(), "p95 requires at least one measurement");
    samples.sort_unstable();
    let index = samples
        .len()
        .saturating_mul(95)
        .div_ceil(100)
        .saturating_sub(1);
    samples[index]
}

#[tokio::test]
#[ignore = "requires the stratum-ontology PostgreSQL container"]
async fn migration_creates_only_the_canonical_tables_and_relational_constraints_hold() {
    let _guard = integration_lock().lock().await;
    let store = connect_store().await;
    assert!(store.is_ready().await);
    let pool = connect_pool().await;

    let table_names = sqlx::query_scalar::<_, String>(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = 'public' \
         AND (table_name = 'ontologies' OR table_name LIKE 'ontology_%')",
    )
    .fetch_all(&pool)
    .await
    .expect("canonical tables should be queryable");
    let expected_tables = [
        "ontologies",
        "ontology_object_types",
        "ontology_properties",
        "ontology_link_types",
        "ontology_canvas_positions",
    ];
    assert_eq!(table_names.len(), expected_tables.len());
    for expected in expected_tables {
        assert!(
            table_names.iter().any(|name| name == expected),
            "missing canonical table {expected}"
        );
    }

    let left = store
        .create(create_input("constraint_left"))
        .await
        .expect("left root should be created");
    let right = store
        .create(create_input("constraint_right"))
        .await
        .expect("right root should be created");
    let left_object_id = ObjectTypeId::new();
    let right_object_id = ObjectTypeId::new();
    for (id, ontology_id, name) in [
        (left_object_id, left.ontology.id, "left_object"),
        (right_object_id, right.ontology.id, "right_object"),
    ] {
        sqlx::query(
            "INSERT INTO ontology_object_types \
             (id, ontology_id, name, display_name, description, sort_order) \
             VALUES ($1, $2, $3, $4, NULL, 0)",
        )
        .bind(id.as_uuid())
        .bind(ontology_id.as_uuid())
        .bind(name)
        .bind(name)
        .execute(&pool)
        .await
        .expect("fixture object type should be inserted");
    }

    let cross_ontology_link = sqlx::query(
        "INSERT INTO ontology_link_types \
         (id, ontology_id, name, display_name, description, source_object_type_id, \
          target_object_type_id, source_to_target, target_to_source, sort_order) \
         VALUES ($1, $2, 'cross_ontology', 'Cross ontology', NULL, $3, $4, 'many', 'many', 0)",
    )
    .bind(LinkTypeId::new().as_uuid())
    .bind(left.ontology.id.as_uuid())
    .bind(left_object_id.as_uuid())
    .bind(right_object_id.as_uuid())
    .execute(&pool)
    .await;
    assert!(
        cross_ontology_link.is_err(),
        "the composite endpoint foreign key must reject cross-ontology links"
    );

    let invalid_value_type = sqlx::query(
        "INSERT INTO ontology_properties \
         (id, object_type_id, name, display_name, description, value_type, required, sort_order) \
         VALUES ($1, $2, 'invalid_value', 'Invalid value', NULL, 'unsupported', false, 0)",
    )
    .bind(PropertyId::new().as_uuid())
    .bind(left_object_id.as_uuid())
    .execute(&pool)
    .await;
    assert!(
        invalid_value_type.is_err(),
        "the scalar value-type check must reject unsupported values"
    );
}

#[tokio::test]
#[ignore = "requires the stratum-ontology PostgreSQL container"]
async fn crud_list_round_trips_order_and_hard_deletes_children() {
    let _guard = integration_lock().lock().await;
    let store = connect_store().await;
    let pool = connect_pool().await;
    let total_before = store
        .list(ListOntologies::default())
        .await
        .expect("initial list should succeed")
        .total;

    let first = store
        .create(create_input("crud_a"))
        .await
        .expect("first root should be created");
    let second = store
        .create(create_input("crud_b"))
        .await
        .expect("second root should be created");
    assert_eq!(first.revision, 1);
    assert!(first.created_at <= first.updated_at);
    assert_eq!(
        store
            .get(first.ontology.id)
            .await
            .expect("empty root should be readable"),
        Some(first.clone())
    );

    let candidate = ordered_document(&first.ontology);
    let revision = store
        .replace(&candidate, first.revision)
        .await
        .expect("complete ordered document should be stored");
    assert_eq!(revision, 2);
    let loaded = store
        .get(first.ontology.id)
        .await
        .expect("stored document should be readable")
        .expect("stored root should exist");
    assert_eq!(loaded.ontology, candidate);
    assert_eq!(loaded.revision, revision);
    assert!(loaded.updated_at >= loaded.created_at);

    let listed = store
        .list(ListOntologies {
            page: 1,
            per_page: 100,
            sort: ListSort::NameAsc,
        })
        .await
        .expect("list should succeed");
    assert_eq!(listed.total, total_before + 2);
    let first_index = listed
        .data
        .iter()
        .position(|summary| summary.id == first.ontology.id)
        .expect("first root should appear in the list");
    let second_index = listed
        .data
        .iter()
        .position(|summary| summary.id == second.ontology.id)
        .expect("second root should appear in the list");
    assert!(
        first_index < second_index,
        "name sort should be deterministic"
    );
    let out_of_range = store
        .list(ListOntologies {
            page: u32::MAX,
            per_page: 100,
            sort: ListSort::NameAsc,
        })
        .await
        .expect("out-of-range list should succeed");
    assert!(out_of_range.data.is_empty());
    assert_eq!(out_of_range.total, listed.total);

    assert!(matches!(
        store.delete(first.ontology.id, first.revision).await,
        Err(OntologyStoreError::Stale)
    ));
    assert_eq!(
        store
            .get(first.ontology.id)
            .await
            .expect("stale delete must leave root readable")
            .expect("stale delete must leave root present")
            .ontology,
        candidate
    );
    store
        .delete(first.ontology.id, revision)
        .await
        .expect("current revision should hard-delete the aggregate");
    assert!(
        store
            .get(first.ontology.id)
            .await
            .expect("deleted root lookup should succeed")
            .is_none()
    );
    for object_type in &candidate.object_types {
        let object_count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*)::bigint FROM ontology_object_types WHERE id = $1",
        )
        .bind(object_type.id.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("object count should be queryable");
        assert_eq!(object_count, 0);
        for property in &object_type.properties {
            let property_count = sqlx::query_scalar::<_, i64>(
                "SELECT count(*)::bigint FROM ontology_properties WHERE id = $1",
            )
            .bind(property.id.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("property count should be queryable");
            assert_eq!(property_count, 0);
        }
    }
    for link_type in &candidate.link_types {
        let link_count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*)::bigint FROM ontology_link_types WHERE id = $1",
        )
        .bind(link_type.id.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("link count should be queryable");
        assert_eq!(link_count, 0);
    }
}

#[tokio::test]
#[ignore = "requires the stratum-ontology PostgreSQL container"]
async fn store_reports_validation_name_and_typed_identity_conflicts_without_mutation() {
    let _guard = integration_lock().lock().await;
    let store = connect_store().await;

    let invalid_create = store
        .create(CreateOntology {
            name: "Not-valid".to_owned(),
            display_name: "Invalid".to_owned(),
            description: None,
        })
        .await
        .expect_err("invalid root metadata must fail validation");
    assert!(matches!(
        invalid_create,
        OntologyStoreError::Validation(OntologyError::Validation { .. })
    ));

    let shared_name = unique_name("name_conflict");
    store
        .create(CreateOntology {
            name: shared_name.clone(),
            display_name: "First".to_owned(),
            description: None,
        })
        .await
        .expect("first name claim should succeed");
    assert!(matches!(
        store
            .create(CreateOntology {
                name: shared_name,
                display_name: "Second".to_owned(),
                description: None,
            })
            .await,
        Err(OntologyStoreError::NameConflict { .. })
    ));

    let scoped_root = store
        .create(create_input("scope"))
        .await
        .expect("scope root should be created");
    let scoped_document = ordered_document(&scoped_root.ontology);
    let scoped_revision = store
        .replace(&scoped_document, scoped_root.revision)
        .await
        .expect("properties with the same name under different owners are valid");
    assert_eq!(scoped_document.object_types[0].properties[1].name, "name");
    assert_eq!(scoped_document.object_types[1].properties[0].name, "name");

    let mut duplicate_property = scoped_document.clone();
    duplicate_property.object_types[0]
        .properties
        .push(Property {
            id: PropertyId::new(),
            name: "email".to_owned(),
            display_name: "Other email".to_owned(),
            description: None,
            value_type: ValueType::String,
            required: false,
        });
    assert!(matches!(
        store.replace(&duplicate_property, scoped_revision).await,
        Err(OntologyStoreError::Validation(_))
    ));
    let mut duplicate_object_name = scoped_document.clone();
    duplicate_object_name.object_types[1].name = duplicate_object_name.object_types[0].name.clone();
    assert!(matches!(
        store.replace(&duplicate_object_name, scoped_revision).await,
        Err(OntologyStoreError::Validation(_))
    ));
    let mut duplicate_link_name = scoped_document.clone();
    duplicate_link_name.link_types.push(LinkType {
        id: LinkTypeId::new(),
        name: scoped_document.link_types[0].name.clone(),
        display_name: "Duplicate link".to_owned(),
        description: None,
        source_object_type_id: scoped_document.object_types[0].id,
        target_object_type_id: scoped_document.object_types[1].id,
        source_to_target: Cardinality::One,
        target_to_source: Cardinality::Many,
    });
    assert!(matches!(
        store.replace(&duplicate_link_name, scoped_revision).await,
        Err(OntologyStoreError::Validation(_))
    ));
    assert_eq!(
        store
            .get(scoped_root.ontology.id)
            .await
            .expect("validation failure should preserve data")
            .expect("scoped root should remain present")
            .ontology,
        scoped_document
    );

    let object_owner = store
        .create(create_input("object_owner"))
        .await
        .expect("object owner should be created");
    let object_owner_document = one_object_document(
        &object_owner.ontology,
        ObjectTypeId::new(),
        PropertyId::new(),
    );
    store
        .replace(&object_owner_document, object_owner.revision)
        .await
        .expect("object owner document should be stored");
    let object_conflict_target = store
        .create(create_input("object_conflict_target"))
        .await
        .expect("object conflict target should be created");
    let object_conflict = one_object_document(
        &object_conflict_target.ontology,
        object_owner_document.object_types[0].id,
        PropertyId::new(),
    );
    assert!(matches!(
        store
            .replace(&object_conflict, object_conflict_target.revision)
            .await,
        Err(OntologyStoreError::EntityIdConflict { .. })
    ));

    let property_owner = store
        .create(create_input("property_owner"))
        .await
        .expect("property owner should be created");
    let property_owner_document = one_object_document(
        &property_owner.ontology,
        ObjectTypeId::new(),
        PropertyId::new(),
    );
    store
        .replace(&property_owner_document, property_owner.revision)
        .await
        .expect("property owner document should be stored");
    let property_conflict_target = store
        .create(create_input("property_conflict_target"))
        .await
        .expect("property conflict target should be created");
    let property_conflict = one_object_document(
        &property_conflict_target.ontology,
        ObjectTypeId::new(),
        property_owner_document.object_types[0].properties[0].id,
    );
    assert!(matches!(
        store
            .replace(&property_conflict, property_conflict_target.revision)
            .await,
        Err(OntologyStoreError::EntityIdConflict { .. })
    ));

    let link_owner = store
        .create(create_input("link_owner"))
        .await
        .expect("link owner should be created");
    let link_owner_document = linked_document(
        &link_owner.ontology,
        ObjectTypeId::new(),
        ObjectTypeId::new(),
        LinkTypeId::new(),
    );
    store
        .replace(&link_owner_document, link_owner.revision)
        .await
        .expect("link owner document should be stored");
    let link_conflict_target = store
        .create(create_input("link_conflict_target"))
        .await
        .expect("link conflict target should be created");
    let link_conflict = linked_document(
        &link_conflict_target.ontology,
        ObjectTypeId::new(),
        ObjectTypeId::new(),
        link_owner_document.link_types[0].id,
    );
    assert!(matches!(
        store
            .replace(&link_conflict, link_conflict_target.revision)
            .await,
        Err(OntologyStoreError::EntityIdConflict { .. })
    ));
    for root in [
        object_conflict_target,
        property_conflict_target,
        link_conflict_target,
    ] {
        let stored = store
            .get(root.ontology.id)
            .await
            .expect("conflict target should be readable")
            .expect("conflict target should remain present");
        assert_eq!(stored.revision, root.revision);
        assert!(stored.ontology.object_types.is_empty());
    }

    let missing_root = Ontology {
        id: OntologyId::new(),
        name: unique_name("missing"),
        display_name: "Missing root".to_owned(),
        description: None,
        object_types: Vec::new(),
        link_types: Vec::new(),
        canvas: Canvas {
            positions: Vec::new(),
        },
    };
    let missing_document =
        one_object_document(&missing_root, ObjectTypeId::new(), PropertyId::new());
    assert!(matches!(
        store.replace(&missing_document, 1).await,
        Err(OntologyStoreError::NotFound)
    ));
    assert!(matches!(
        store
            .neighborhood(scoped_root.ontology.id, ObjectTypeId::new(), 1,)
            .await,
        Err(OntologyStoreError::ObjectTypeNotFound)
    ));
    assert!(matches!(
        store
            .neighborhood(
                scoped_root.ontology.id,
                scoped_document.object_types[0].id,
                6
            )
            .await,
        Err(OntologyStoreError::InvalidDepth)
    ));
    assert!(matches!(
        store
            .neighborhood(OntologyId::new(), scoped_document.object_types[0].id, 0)
            .await,
        Err(OntologyStoreError::NotFound)
    ));
}

#[tokio::test]
#[ignore = "requires the stratum-ontology PostgreSQL container"]
async fn cas_failures_roll_back_and_concurrent_writes_only_admit_current_revisions() {
    let _guard = integration_lock().lock().await;
    let store = connect_store().await;

    let target = store
        .create(create_input("rollback_target"))
        .await
        .expect("rollback target should be created");
    let original = one_object_document(&target.ontology, ObjectTypeId::new(), PropertyId::new());
    let original_revision = store
        .replace(&original, target.revision)
        .await
        .expect("original target document should be stored");
    let blocker = store
        .create(create_input("rollback_blocker"))
        .await
        .expect("blocker should be created");
    let blocker_document =
        one_object_document(&blocker.ontology, ObjectTypeId::new(), PropertyId::new());
    store
        .replace(&blocker_document, blocker.revision)
        .await
        .expect("blocker document should be stored");
    let colliding_replacement = one_object_document(
        &target.ontology,
        blocker_document.object_types[0].id,
        PropertyId::new(),
    );
    assert!(matches!(
        store
            .replace(&colliding_replacement, original_revision)
            .await,
        Err(OntologyStoreError::EntityIdConflict { .. })
    ));
    let after_rollback = store
        .get(target.ontology.id)
        .await
        .expect("target should remain readable")
        .expect("target should remain present");
    assert_eq!(after_rollback.ontology, original);
    assert_eq!(after_rollback.revision, original_revision);

    let stale_root = store
        .create(create_input("stale"))
        .await
        .expect("stale root should be created");
    let current = one_object_document(&stale_root.ontology, ObjectTypeId::new(), PropertyId::new());
    let current_revision = store
        .replace(&current, stale_root.revision)
        .await
        .expect("current document should be stored");
    let mut stale =
        one_object_document(&stale_root.ontology, ObjectTypeId::new(), PropertyId::new());
    stale.display_name = "Stale write".to_owned();
    assert!(matches!(
        store.replace(&stale, stale_root.revision).await,
        Err(OntologyStoreError::Stale)
    ));
    assert_eq!(
        store
            .get(stale_root.ontology.id)
            .await
            .expect("stale root should be readable")
            .expect("stale root should remain present")
            .ontology,
        current
    );
    assert_eq!(current_revision, 2);

    let same_revision_root = store
        .create(create_input("same_revision"))
        .await
        .expect("same-revision root should be created");
    let first_candidate = one_object_document(
        &same_revision_root.ontology,
        ObjectTypeId::new(),
        PropertyId::new(),
    );
    let mut second_candidate = one_object_document(
        &same_revision_root.ontology,
        ObjectTypeId::new(),
        PropertyId::new(),
    );
    second_candidate.display_name = "Other concurrent write".to_owned();
    let first_store = store.clone();
    let second_store = store.clone();
    let (first_result, second_result) = tokio::join!(
        first_store.replace(&first_candidate, same_revision_root.revision),
        second_store.replace(&second_candidate, same_revision_root.revision),
    );
    let results = [first_result, second_result];
    let successful_writes = results
        .iter()
        .filter(|result| result.as_ref().is_ok_and(|revision| *revision == 2))
        .count();
    let stale_writes = results
        .iter()
        .filter(|result| {
            result
                .as_ref()
                .is_err_and(|error| matches!(error, OntologyStoreError::Stale))
        })
        .count();
    assert_eq!(successful_writes, 1);
    assert_eq!(stale_writes, 1);
    assert_eq!(
        store
            .get(same_revision_root.ontology.id)
            .await
            .expect("same-revision root should be readable")
            .expect("same-revision root should remain present")
            .revision,
        2
    );

    let left_root = store
        .create(create_input("independent_left"))
        .await
        .expect("left root should be created");
    let right_root = store
        .create(create_input("independent_right"))
        .await
        .expect("right root should be created");
    let left_document =
        one_object_document(&left_root.ontology, ObjectTypeId::new(), PropertyId::new());
    let right_document =
        one_object_document(&right_root.ontology, ObjectTypeId::new(), PropertyId::new());
    let left_store = store.clone();
    let right_store = store.clone();
    let (left_result, right_result) = tokio::join!(
        left_store.replace(&left_document, left_root.revision),
        right_store.replace(&right_document, right_root.revision),
    );
    assert_eq!(
        left_result.expect("left root should not share a CAS gate"),
        2
    );
    assert_eq!(
        right_result.expect("right root should not share a CAS gate"),
        2
    );
}

#[tokio::test]
#[ignore = "requires the stratum-ontology PostgreSQL container"]
async fn repeatable_reads_are_complete_and_neighborhoods_are_bidirectional_induced_and_ordered() {
    let _guard = integration_lock().lock().await;
    let store = connect_store().await;

    let snapshot_root = store
        .create(create_input("snapshot"))
        .await
        .expect("snapshot root should be created");
    let old_document = one_object_document(
        &snapshot_root.ontology,
        ObjectTypeId::new(),
        PropertyId::new(),
    );
    let old_revision = store
        .replace(&old_document, snapshot_root.revision)
        .await
        .expect("old document should be stored");
    let mut new_document = ordered_document(&snapshot_root.ontology);
    new_document.display_name = "New complete document".to_owned();
    let reader_old = old_document.clone();
    let reader_new = new_document.clone();
    let writer_old = old_document.clone();
    let writer_new = new_document.clone();
    let writer_store = store.clone();
    let snapshot_id = snapshot_root.ontology.id;
    let writer = tokio::spawn(async move {
        let mut revision = old_revision;
        for iteration in 0..32 {
            let document = if iteration % 2 == 0 {
                &writer_new
            } else {
                &writer_old
            };
            revision = writer_store.replace(document, revision).await?;
            tokio::task::yield_now().await;
        }
        Ok::<i64, OntologyStoreError>(revision)
    });
    let mut reads_while_writing = 0_usize;
    while !writer.is_finished() {
        let observed = store
            .get(snapshot_id)
            .await
            .expect("concurrent read should succeed")
            .expect("concurrent read should retain the root");
        assert!(
            observed.ontology == reader_old || observed.ontology == reader_new,
            "a repeatable read must never assemble a mixed aggregate"
        );
        reads_while_writing = reads_while_writing.saturating_add(1);
    }
    let final_revision = writer
        .await
        .expect("writer task should not panic")
        .expect("writer should not fail");
    assert_eq!(final_revision, 34);
    assert!(reads_while_writing > 0);

    let neighborhood_snapshot_root = store
        .create(create_input("neighborhood_snapshot"))
        .await
        .expect("neighborhood snapshot root should be created");
    let neighborhood_fixture = traversal_fixture(&neighborhood_snapshot_root.ontology);
    let neighborhood_old = neighborhood_fixture.document;
    let neighborhood_old_revision = store
        .replace(&neighborhood_old, neighborhood_snapshot_root.revision)
        .await
        .expect("old neighborhood document should be stored");
    let expected_old_neighborhood = store
        .neighborhood(
            neighborhood_snapshot_root.ontology.id,
            neighborhood_fixture.alpha,
            2,
        )
        .await
        .expect("old neighborhood should be readable");
    let mut neighborhood_new = neighborhood_old.clone();
    for object_type in &mut neighborhood_new.object_types {
        object_type.display_name.push_str(" updated");
        for property in &mut object_type.properties {
            property.required = !property.required;
        }
    }
    for position in &mut neighborhood_new.canvas.positions {
        position.x += 100.0;
    }
    let neighborhood_new_revision = store
        .replace(&neighborhood_new, neighborhood_old_revision)
        .await
        .expect("new neighborhood document should be stored");
    let expected_new_neighborhood = store
        .neighborhood(
            neighborhood_snapshot_root.ontology.id,
            neighborhood_fixture.alpha,
            2,
        )
        .await
        .expect("new neighborhood should be readable");
    assert_ne!(expected_old_neighborhood, expected_new_neighborhood);
    let neighborhood_start_revision = store
        .replace(&neighborhood_old, neighborhood_new_revision)
        .await
        .expect("old neighborhood document should be restored");

    let writer_old = neighborhood_old.clone();
    let writer_new = neighborhood_new.clone();
    let writer_store = store.clone();
    let neighborhood_id = neighborhood_snapshot_root.ontology.id;
    let neighborhood_origin = neighborhood_fixture.alpha;
    let writer = tokio::spawn(async move {
        let mut revision = neighborhood_start_revision;
        for iteration in 0..32 {
            let document = if iteration % 2 == 0 {
                &writer_new
            } else {
                &writer_old
            };
            revision = writer_store.replace(document, revision).await?;
            tokio::task::yield_now().await;
        }
        Ok::<i64, OntologyStoreError>(revision)
    });
    let mut neighborhoods_while_writing = 0_usize;
    while !writer.is_finished() {
        let observed = store
            .neighborhood(neighborhood_id, neighborhood_origin, 2)
            .await
            .expect("concurrent neighborhood should succeed");
        assert!(
            observed == expected_old_neighborhood || observed == expected_new_neighborhood,
            "a repeatable read must never assemble a mixed neighborhood"
        );
        neighborhoods_while_writing = neighborhoods_while_writing.saturating_add(1);
    }
    let final_revision = writer
        .await
        .expect("neighborhood writer task should not panic")
        .expect("neighborhood writer should not fail");
    assert_eq!(final_revision, 36);
    assert!(neighborhoods_while_writing > 0);

    let traversal_root = store
        .create(create_input("traversal"))
        .await
        .expect("traversal root should be created");
    let fixture = traversal_fixture(&traversal_root.ontology);
    store
        .replace(&fixture.document, traversal_root.revision)
        .await
        .expect("traversal graph should be stored");

    let depth_zero = store
        .neighborhood(traversal_root.ontology.id, fixture.alpha, 0)
        .await
        .expect("depth-zero neighborhood should be readable");
    assert_eq!(
        depth_zero
            .object_types
            .iter()
            .map(|object_type| object_type.id)
            .collect::<Vec<_>>(),
        vec![fixture.alpha]
    );
    assert_eq!(
        depth_zero
            .link_types
            .iter()
            .map(|link_type| link_type.id)
            .collect::<Vec<_>>(),
        vec![fixture.self_link]
    );
    assert_eq!(
        depth_zero
            .canvas
            .positions
            .iter()
            .map(|position| position.object_type_id)
            .collect::<Vec<_>>(),
        vec![fixture.alpha]
    );

    let depth_one = store
        .neighborhood(traversal_root.ontology.id, fixture.alpha, 1)
        .await
        .expect("depth-one neighborhood should be readable");
    assert_eq!(depth_one.origin_object_type_id, fixture.alpha);
    assert_eq!(depth_one.depth, 1);
    assert_eq!(
        depth_one
            .object_types
            .iter()
            .map(|object_type| object_type.id)
            .collect::<Vec<_>>(),
        vec![fixture.charlie, fixture.alpha, fixture.bravo]
    );
    assert!(
        depth_one
            .object_types
            .iter()
            .all(|object_type| object_type.properties.len() == 1)
    );
    assert_eq!(
        depth_one
            .link_types
            .iter()
            .map(|link_type| link_type.id)
            .collect::<Vec<_>>(),
        vec![
            fixture.self_link,
            fixture.reverse_link,
            fixture.alpha_to_charlie,
            fixture.induced_link,
        ]
    );
    assert_eq!(
        depth_one
            .canvas
            .positions
            .iter()
            .map(|position| position.object_type_id)
            .collect::<Vec<_>>(),
        vec![fixture.bravo, fixture.alpha]
    );

    let depth_two = store
        .neighborhood(traversal_root.ontology.id, fixture.alpha, 2)
        .await
        .expect("depth-two neighborhood should be readable");
    assert_eq!(
        depth_two
            .object_types
            .iter()
            .map(|object_type| object_type.id)
            .collect::<Vec<_>>(),
        vec![fixture.charlie, fixture.alpha, fixture.delta, fixture.bravo]
    );
    assert_eq!(
        depth_two
            .link_types
            .iter()
            .map(|link_type| link_type.id)
            .collect::<Vec<_>>(),
        fixture.all_links
    );
    assert_eq!(
        depth_two
            .canvas
            .positions
            .iter()
            .map(|position| position.object_type_id)
            .collect::<Vec<_>>(),
        vec![fixture.bravo, fixture.alpha, fixture.delta]
    );
}

#[tokio::test]
#[ignore = "requires the stratum-ontology PostgreSQL container"]
async fn maximum_fixture_full_and_depth_five_reads_have_a_p95_at_or_below_100ms() {
    let _guard = integration_lock().lock().await;
    let store = connect_store().await;
    let root = store
        .create(create_input("maximum"))
        .await
        .expect("maximum fixture root should be created");
    let candidate = maximum_document(&root.ontology);
    let revision = store
        .replace(&candidate, root.revision)
        .await
        .expect("maximum fixture should be stored");
    assert_eq!(revision, 2);
    let origin = candidate.object_types[0].id;

    let depth_four = store
        .neighborhood(root.ontology.id, origin, 4)
        .await
        .expect("depth-four fixture check should succeed");
    assert_eq!(depth_four.object_types.len(), 401);

    for _ in 0..3 {
        let full = store
            .get(root.ontology.id)
            .await
            .expect("warm-up full read should succeed")
            .expect("maximum root should exist");
        assert_eq!(full.ontology.object_types.len(), MAX_OBJECT_TYPES);
        let neighborhood = store
            .neighborhood(root.ontology.id, origin, 5)
            .await
            .expect("warm-up depth-five read should succeed");
        assert_eq!(neighborhood.object_types.len(), MAX_OBJECT_TYPES);
    }

    let mut full_samples = Vec::with_capacity(MEASUREMENTS);
    let mut depth_five_samples = Vec::with_capacity(MEASUREMENTS);
    for _ in 0..MEASUREMENTS {
        let started = Instant::now();
        let full = store
            .get(root.ontology.id)
            .await
            .expect("measured full read should succeed")
            .expect("maximum root should exist");
        full_samples.push(started.elapsed());
        assert_eq!(full.ontology.object_types.len(), MAX_OBJECT_TYPES);
        assert_eq!(
            full.ontology
                .object_types
                .iter()
                .map(|object_type| object_type.properties.len())
                .sum::<usize>(),
            MAX_OBJECT_TYPES * MAX_PROPERTIES_PER_OBJECT_TYPE
        );
        assert_eq!(full.ontology.link_types.len(), MAX_LINK_TYPES);
        assert_eq!(full.ontology.canvas.positions.len(), MAX_OBJECT_TYPES);

        let started = Instant::now();
        let neighborhood = store
            .neighborhood(root.ontology.id, origin, 5)
            .await
            .expect("measured depth-five read should succeed");
        depth_five_samples.push(started.elapsed());
        assert_eq!(neighborhood.object_types.len(), MAX_OBJECT_TYPES);
        assert_eq!(neighborhood.link_types.len(), MAX_LINK_TYPES);
        assert_eq!(neighborhood.canvas.positions.len(), MAX_OBJECT_TYPES);
    }

    let full_p95 = p95(&mut full_samples);
    let depth_five_p95 = p95(&mut depth_five_samples);
    println!("maximum fixture p95: full={full_p95:?}, depth_five={depth_five_p95:?}");
    let target = Duration::from_millis(100);
    assert!(
        full_p95 <= target,
        "full graph p95 {full_p95:?} exceeded {target:?}"
    );
    assert!(
        depth_five_p95 <= target,
        "depth-five neighborhood p95 {depth_five_p95:?} exceeded {target:?}"
    );
}
