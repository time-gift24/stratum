use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde::de::DeserializeOwned;
use tower::ServiceExt;
use wyse_filesystem::{
    CasExpectation, DirEntry, Entry, FileMetadata, Filesystem, FilesystemError, RecordVersion,
    VersionedEntry, VirtualPath,
};
use wyse_ontology::{
    Cardinality, DraftName, FilesystemDraftStore, LinkCardinalityConstraint, LinkId, LinkRecord,
    LinkType, LinkTypeId, NewLinkRecord, NewObjectRecord, ObjectId, ObjectRecord, ObjectType,
    ObjectTypeId, OntologyError, OntologyRepository, Page, PropertyType, PropertyTypeId,
    PublishedRevision, RevisionId, SchemaDocument, SchemaValidationSnapshot, TagName, ValueType,
};
use wyse_ontology_api::router;

#[tokio::test]
async fn graph_route_returns_schema_nodes_and_edges() -> Result<(), Box<dyn std::error::Error>> {
    let app = router(test_service_with_online_schema().await?);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/ontology/graph?tag=online")
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = decode_json(response).await?;
    assert_eq!(body["nodes"].as_array().map(Vec::len), Some(2));
    assert_eq!(body["edges"].as_array().map(Vec::len), Some(1));
    Ok(())
}

#[tokio::test]
async fn create_draft_returns_an_etag() -> Result<(), Box<dyn std::error::Error>> {
    let app = router(test_service_with_online_schema().await?);
    let response = app
        .oneshot(Request::builder().method("POST").uri("/v1/ontology/drafts").header("content-type", "application/json").body(Body::from(r#"{"name":"experiment","schema":{"schema_version":1,"object_types":[],"link_types":[]}}"#))?)
        .await?;

    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(response.headers().contains_key("etag"));
    Ok(())
}

#[tokio::test]
async fn creating_an_existing_draft_returns_conflict() -> Result<(), Box<dyn std::error::Error>> {
    let app = router(test_service_with_online_schema().await?);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/ontology/drafts")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"name":"main","schema":{"schema_version":1,"object_types":[],"link_types":[]}}"#,
                ))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    Ok(())
}

#[tokio::test]
async fn current_if_match_allows_a_schema_mutation() -> Result<(), Box<dyn std::error::Error>> {
    let app = router(test_service_with_online_schema().await?);
    let draft = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/ontology/drafts/main")
                .body(Body::empty())?,
        )
        .await?;
    let etag = draft.headers()["etag"].clone();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/ontology/drafts/main/object-types")
                .header("content-type", "application/json")
                .header("if-match", etag)
                .body(Body::from(r#"{"name":"Project"}"#))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key("etag"));
    let body: serde_json::Value = decode_json(response).await?;
    assert_eq!(
        body["schema"]["object_types"].as_array().map(Vec::len),
        Some(3)
    );
    Ok(())
}

#[tokio::test]
async fn deleting_a_draft_with_a_stale_if_match_returns_precondition_failed()
-> Result<(), Box<dyn std::error::Error>> {
    let app = router(test_service_with_online_schema().await?);
    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/v1/ontology/drafts/main")
                .header(
                    "if-match",
                    "\"0000000000000000000000000000000000000000000000000000000000000000\"",
                )
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
    Ok(())
}

#[tokio::test]
async fn deleting_the_online_tag_returns_conflict() -> Result<(), Box<dyn std::error::Error>> {
    let app = router(test_service_with_online_schema().await?);
    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/v1/ontology/tags/online")
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    Ok(())
}

#[tokio::test]
async fn validate_returns_unprocessable_entity_for_an_invalid_static_schema()
-> Result<(), Box<dyn std::error::Error>> {
    let app = router(test_service_with_online_schema().await?);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/ontology/drafts/bad/validate")
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    Ok(())
}

async fn decode_json<T: DeserializeOwned>(
    response: axum::response::Response,
) -> Result<T, Box<dyn std::error::Error>> {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn test_service_with_online_schema()
-> Result<Arc<wyse_ontology::OntologyService>, Box<dyn std::error::Error>> {
    let person = ObjectTypeId::new();
    let company = ObjectTypeId::new();
    let schema = SchemaDocument {
        schema_version: 1,
        object_types: vec![
            object_type(person, "Person"),
            object_type(company, "Company"),
        ],
        link_types: vec![LinkType::new(
            LinkTypeId::new(),
            "works_for".to_owned(),
            person,
            company,
            Cardinality::ManyToOne,
        )],
    };
    let filesystem = Arc::new(MemoryFilesystem::default());
    let drafts = FilesystemDraftStore::new(filesystem.clone());
    drafts
        .create(DraftName::try_from("main".to_owned())?, schema.clone())
        .await?;
    let repository = Arc::new(MemoryRepository::default());
    let service = Arc::new(wyse_ontology::OntologyService::new(
        drafts,
        repository.clone(),
    ));
    let revision = service
        .publish(&DraftName::try_from("main".to_owned())?)
        .await?;
    service.put_tag(&TagName::online(), &revision.id).await?;
    let invalid = SchemaDocument {
        schema_version: 0,
        object_types: Vec::new(),
        link_types: Vec::new(),
    };
    let path = VirtualPath::try_from("/ontology/drafts/bad.json")?;
    filesystem
        .put(
            &path,
            Entry::new(serde_json::to_vec(&invalid)?),
            CasExpectation::Absent,
        )
        .await?;
    Ok(service)
}

fn object_type(id: ObjectTypeId, name: &str) -> ObjectType {
    ObjectType {
        id,
        name: name.to_owned(),
        description: String::new(),
        properties: vec![PropertyType {
            id: PropertyTypeId::new(),
            name: "name".to_owned(),
            description: String::new(),
            value_type: ValueType::String,
            required: true,
        }],
    }
}

#[derive(Default)]
struct MemoryFilesystem {
    entries: Mutex<BTreeMap<VirtualPath, VersionedEntry>>,
    next: Mutex<u64>,
}

#[async_trait]
impl Filesystem for MemoryFilesystem {
    async fn get(&self, path: &VirtualPath) -> Result<Option<VersionedEntry>, FilesystemError> {
        Ok(self
            .entries
            .lock()
            .map_err(|_| FilesystemError::UnsupportedCas)?
            .get(path)
            .cloned())
    }
    async fn put(
        &self,
        path: &VirtualPath,
        entry: Entry,
        cas: CasExpectation,
    ) -> Result<RecordVersion, FilesystemError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| FilesystemError::UnsupportedCas)?;
        let matches = match cas {
            CasExpectation::Absent => !entries.contains_key(path),
            CasExpectation::Version(expected) => entries
                .get(path)
                .is_some_and(|current| current.version == expected),
            CasExpectation::Any => true,
        };
        if !matches {
            return Err(FilesystemError::VersionMismatch { path: path.clone() });
        }
        let mut next = self
            .next
            .lock()
            .map_err(|_| FilesystemError::UnsupportedCas)?;
        *next += 1;
        let version = RecordVersion::from_backend(*next);
        entries.insert(path.clone(), VersionedEntry { entry, version });
        Ok(version)
    }
    async fn delete(
        &self,
        path: &VirtualPath,
        _cas: CasExpectation,
    ) -> Result<(), FilesystemError> {
        self.entries
            .lock()
            .map_err(|_| FilesystemError::UnsupportedCas)?
            .remove(path);
        Ok(())
    }
    async fn read_file(&self, _: &VirtualPath) -> Result<Vec<u8>, FilesystemError> {
        Err(FilesystemError::UnsupportedCas)
    }
    async fn write_file(&self, _: &VirtualPath, _: Vec<u8>) -> Result<(), FilesystemError> {
        Err(FilesystemError::UnsupportedCas)
    }
    async fn list_dir(&self, _: &VirtualPath) -> Result<Vec<DirEntry>, FilesystemError> {
        Ok(Vec::new())
    }
    async fn metadata(&self, _: &VirtualPath) -> Result<FileMetadata, FilesystemError> {
        Err(FilesystemError::UnsupportedCas)
    }
    async fn create_dir(&self, _: &VirtualPath) -> Result<(), FilesystemError> {
        Err(FilesystemError::UnsupportedCas)
    }
    async fn remove_file(&self, _: &VirtualPath) -> Result<(), FilesystemError> {
        Err(FilesystemError::UnsupportedCas)
    }
    async fn remove_dir(&self, _: &VirtualPath) -> Result<(), FilesystemError> {
        Err(FilesystemError::UnsupportedCas)
    }
}

#[derive(Default)]
struct MemoryRepository {
    revisions: Mutex<BTreeMap<RevisionId, PublishedRevision>>,
    tags: Mutex<BTreeMap<TagName, RevisionId>>,
}

#[async_trait]
impl OntologyRepository for MemoryRepository {
    async fn insert_revision(&self, revision: PublishedRevision) -> Result<(), OntologyError> {
        self.revisions
            .lock()
            .map_err(|_| repository_error())?
            .insert(revision.id.clone(), revision);
        Ok(())
    }
    async fn get_revision(
        &self,
        id: &RevisionId,
    ) -> Result<Option<PublishedRevision>, OntologyError> {
        Ok(self
            .revisions
            .lock()
            .map_err(|_| repository_error())?
            .get(id)
            .cloned())
    }
    async fn list_revisions(&self) -> Result<Vec<PublishedRevision>, OntologyError> {
        Ok(self
            .revisions
            .lock()
            .map_err(|_| repository_error())?
            .values()
            .cloned()
            .collect())
    }
    async fn put_tag(&self, name: &TagName, id: &RevisionId) -> Result<(), OntologyError> {
        self.tags
            .lock()
            .map_err(|_| repository_error())?
            .insert(name.clone(), id.clone());
        Ok(())
    }
    async fn get_tag(&self, name: &TagName) -> Result<Option<RevisionId>, OntologyError> {
        Ok(self
            .tags
            .lock()
            .map_err(|_| repository_error())?
            .get(name)
            .cloned())
    }
    async fn delete_tag(&self, name: &TagName) -> Result<(), OntologyError> {
        self.tags
            .lock()
            .map_err(|_| repository_error())?
            .remove(name);
        Ok(())
    }
    async fn schema_validation_snapshot(&self) -> Result<SchemaValidationSnapshot, OntologyError> {
        Ok(SchemaValidationSnapshot {
            objects: Vec::new(),
            links: Vec::new(),
        })
    }
    async fn create_object(&self, _: NewObjectRecord) -> Result<ObjectRecord, OntologyError> {
        Err(repository_error())
    }
    async fn get_object(&self, _: ObjectId) -> Result<Option<ObjectRecord>, OntologyError> {
        Ok(None)
    }
    async fn page_objects(
        &self,
        _: ObjectTypeId,
        _: Option<ObjectId>,
        _: u32,
    ) -> Result<Page<ObjectRecord>, OntologyError> {
        Err(repository_error())
    }
    async fn replace_object(&self, _: ObjectRecord) -> Result<ObjectRecord, OntologyError> {
        Err(repository_error())
    }
    async fn delete_object(&self, _: ObjectId, _: u64, _: bool) -> Result<(), OntologyError> {
        Err(repository_error())
    }
    async fn create_link_with_cardinality(
        &self,
        _: NewLinkRecord,
        _: &[LinkCardinalityConstraint],
    ) -> Result<LinkRecord, OntologyError> {
        Err(repository_error())
    }
    async fn get_link(&self, _: LinkId) -> Result<Option<LinkRecord>, OntologyError> {
        Ok(None)
    }
    async fn page_links(
        &self,
        _: Option<LinkId>,
        _: u32,
    ) -> Result<Page<LinkRecord>, OntologyError> {
        Err(repository_error())
    }
    async fn replace_link_with_cardinality(
        &self,
        _: LinkRecord,
        _: &[LinkCardinalityConstraint],
    ) -> Result<LinkRecord, OntologyError> {
        Err(repository_error())
    }
    async fn delete_link(&self, _: LinkId, _: u64) -> Result<(), OntologyError> {
        Err(repository_error())
    }
}

fn repository_error() -> OntologyError {
    OntologyError::Repository("test repository lock poisoned".into())
}
