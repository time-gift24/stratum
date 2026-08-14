//! Canonical Ontology metadata domain and PostgreSQL persistence.
//!
//! This library owns the Ontology aggregate, its deterministic validation, and
//! the five PostgreSQL tables that persist it. HTTP DTOs and process startup
//! remain in `stratum-api`.

mod domain;
mod error;
mod id;
mod store;
mod validation;

pub use domain::{
    Canvas, CanvasPosition, Cardinality, CreateOntology, LinkType, ListOntologies, ListSort,
    Neighborhood, ObjectType, Ontology, OntologyListPage, OntologyRecord, OntologySummary,
    Property, ValueType,
};
pub use error::{IdParseError, OntologyError, OntologyStoreError};
pub use id::{LinkTypeId, ObjectTypeId, OntologyId, PropertyId};
pub use store::OntologyStore;
pub use validation::{Violation, ViolationCode, validate};
