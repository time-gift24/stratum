//! Domain types and validation for the Wyse ontology service.

pub mod draft;
pub mod error;
pub mod id;
pub mod schema;

pub use draft::{Draft, FilesystemDraftStore, canonical_schema_bytes, revision_id};
pub use error::OntologyError;
pub use id::{
    DraftName, LinkId, LinkTypeId, ObjectId, ObjectTypeId, PropertyTypeId, RevisionId, SchemaRef,
    TagName,
};
pub use schema::{Cardinality, LinkType, ObjectType, PropertyType, SchemaDocument, ValueType};
