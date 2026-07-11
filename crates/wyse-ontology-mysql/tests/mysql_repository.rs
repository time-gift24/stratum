use std::sync::Arc;

use serde_json::Map;
use tokio::sync::Barrier;
use wyse_ontology::{
    Cardinality, LinkCardinalityConstraint, LinkId, LinkRecord, LinkTypeId, NewLinkRecord,
    NewObjectRecord, ObjectId, ObjectType, ObjectTypeId, OntologyError, OntologyRepository,
    PublishedRevision, RevisionId, SchemaDocument, TagName,
};
use wyse_ontology_mysql::SqlxOntologyRepository;

fn published_revision() -> PublishedRevision {
    PublishedRevision {
        id: RevisionId::try_from("a".repeat(64)).expect("valid revision id"),
        schema: SchemaDocument {
            schema_version: 1,
            object_types: vec![ObjectType {
                id: ObjectTypeId::new(),
                name: "person".to_owned(),
                description: String::new(),
                properties: Vec::new(),
            }],
            link_types: Vec::new(),
        },
    }
}

#[tokio::test]
#[ignore = "requires MySQL 8 started by the crate Makefile"]
async fn repository_persists_revision_and_online_tag() -> Result<(), Box<dyn std::error::Error>> {
    use sqlx::MySqlPool;
    use wyse_ontology::OntologyRepository;

    let pool = MySqlPool::connect(&std::env::var("DATABASE_URL")?).await?;
    let repository = SqlxOntologyRepository::new(pool);
    let revision = published_revision();
    let online = TagName::online();

    repository.insert_revision(revision.clone()).await?;
    repository.put_tag(&online, &revision.id).await?;

    assert_eq!(repository.get_tag(&online).await?, Some(revision.id));
    Ok(())
}

#[tokio::test]
#[ignore = "requires MySQL 8 started by the crate Makefile"]
async fn repository_atomically_enforces_cardinality_and_excludes_replaced_link()
-> Result<(), Box<dyn std::error::Error>> {
    use sqlx::MySqlPool;

    let repository = Arc::new(SqlxOntologyRepository::new(
        MySqlPool::connect(&std::env::var("DATABASE_URL")?).await?,
    ));
    let object_type_id = ObjectTypeId::new();
    let source = ObjectId::new();
    let first_target = ObjectId::new();
    let second_target = ObjectId::new();
    for id in [source, first_target, second_target] {
        repository
            .create_object(NewObjectRecord {
                id,
                object_type_id,
                values: Map::new(),
            })
            .await?;
    }

    let link_type_id = LinkTypeId::new();
    let constraints = [LinkCardinalityConstraint {
        cardinality: Cardinality::ManyToOne,
    }];
    let barrier = Arc::new(Barrier::new(3));
    let first = {
        let barrier = barrier.clone();
        let repository = repository.clone();
        async move {
            barrier.wait().await;
            repository
                .create_link_with_cardinality(
                    NewLinkRecord {
                        id: LinkId::new(),
                        link_type_id,
                        source_object_id: source,
                        target_object_id: first_target,
                    },
                    &constraints,
                )
                .await
        }
    };
    let second = {
        let barrier = barrier.clone();
        let repository = repository.clone();
        async move {
            barrier.wait().await;
            repository
                .create_link_with_cardinality(
                    NewLinkRecord {
                        id: LinkId::new(),
                        link_type_id,
                        source_object_id: source,
                        target_object_id: second_target,
                    },
                    &constraints,
                )
                .await
        }
    };

    barrier.wait().await;
    let (first, second) = tokio::join!(first, second);
    let created = match (first, second) {
        (Ok(link), Err(OntologyError::CardinalityConflict { .. }))
        | (Err(OntologyError::CardinalityConflict { .. }), Ok(link)) => link,
        (left, right) => panic!("expected one cardinality conflict, got {left:?} and {right:?}"),
    };

    let replaced = repository
        .replace_link_with_cardinality(
            LinkRecord {
                id: created.id,
                link_type_id: created.link_type_id,
                source_object_id: created.source_object_id,
                target_object_id: created.target_object_id,
                version: created.version,
            },
            &constraints,
        )
        .await?;
    assert_eq!(replaced.version, created.version + 1);
    Ok(())
}
