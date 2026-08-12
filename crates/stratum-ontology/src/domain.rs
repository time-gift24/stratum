//! Ontology aggregate values and store request/response types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{LinkTypeId, ObjectTypeId, OntologyId, PropertyId};

/// An Ontology document without persistence metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct Ontology {
    /// Aggregate identity.
    pub id: OntologyId,
    /// Deployment-wide stable machine name.
    pub name: String,
    /// Human-facing title.
    pub display_name: String,
    /// Optional human-facing description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Ordered Object Types.
    pub object_types: Vec<ObjectType>,
    /// Ordered Link Types.
    pub link_types: Vec<LinkType>,
    /// Canvas layout for Object Types.
    pub canvas: Canvas,
}

/// A named object-type definition and its exclusively owned Properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ObjectType {
    /// Object Type identity.
    pub id: ObjectTypeId,
    /// Ontology-local machine name.
    pub name: String,
    /// Human-facing title.
    pub display_name: String,
    /// Optional human-facing description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Ordered Properties exclusively owned by this Object Type.
    pub properties: Vec<Property>,
}

/// A scalar Property owned by one Object Type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct Property {
    /// Property identity.
    pub id: PropertyId,
    /// Owner-local machine name.
    pub name: String,
    /// Human-facing title.
    pub display_name: String,
    /// Optional human-facing description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Permitted scalar value shape.
    pub value_type: ValueType,
    /// Whether a value is mandatory for future instances.
    pub required: bool,
}

/// A binary semantic relationship between two Object Types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct LinkType {
    /// Link Type identity.
    pub id: LinkTypeId,
    /// Ontology-local machine name.
    pub name: String,
    /// Human-facing title.
    pub display_name: String,
    /// Optional human-facing description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Semantic source endpoint.
    pub source_object_type_id: ObjectTypeId,
    /// Semantic target endpoint.
    pub target_object_type_id: ObjectTypeId,
    /// Cardinality from source to target.
    pub source_to_target: Cardinality,
    /// Cardinality from target to source.
    pub target_to_source: Cardinality,
}

/// Canvas layout stored alongside an Ontology document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct Canvas {
    /// Ordered positions for Object Types.
    pub positions: Vec<CanvasPosition>,
}

/// One Object Type's canvas coordinate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct CanvasPosition {
    /// Positioned Object Type.
    pub object_type_id: ObjectTypeId,
    /// Horizontal coordinate.
    pub x: f64,
    /// Vertical coordinate.
    pub y: f64,
}

/// The scalar value shape of a Property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    /// UTF-8 text.
    String,
    /// Signed integral number.
    Integer,
    /// Floating-point number.
    Number,
    /// Boolean value.
    Boolean,
    /// Calendar date.
    Date,
    /// RFC 3339 date and time.
    DateTime,
}

impl ValueType {
    /// Returns the database representation of this value type.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Date => "date",
            Self::DateTime => "date_time",
        }
    }

    pub(crate) fn from_database(value: &str) -> Option<Self> {
        match value {
            "string" => Some(Self::String),
            "integer" => Some(Self::Integer),
            "number" => Some(Self::Number),
            "boolean" => Some(Self::Boolean),
            "date" => Some(Self::Date),
            "date_time" => Some(Self::DateTime),
            _ => None,
        }
    }
}

/// The cardinality of one direction of a semantic Link Type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cardinality {
    /// Zero or one related endpoint.
    One,
    /// Zero or more related endpoints.
    Many,
}

impl Cardinality {
    /// Returns the database representation of this cardinality.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::One => "one",
            Self::Many => "many",
        }
    }

    pub(crate) fn from_database(value: &str) -> Option<Self> {
        match value {
            "one" => Some(Self::One),
            "many" => Some(Self::Many),
            _ => None,
        }
    }
}

/// Input used to create a new empty Ontology aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateOntology {
    /// Deployment-wide machine name.
    pub name: String,
    /// Human-facing title.
    pub display_name: String,
    /// Optional human-facing description.
    pub description: Option<String>,
}

/// A complete Ontology document plus its persistence metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct OntologyRecord {
    /// Complete current document.
    pub ontology: Ontology,
    /// Internal revision used for compare-and-swap writes.
    pub revision: i64,
    /// Initial creation time.
    pub created_at: DateTime<Utc>,
    /// Time of the most recent successful replacement.
    pub updated_at: DateTime<Utc>,
}

/// One Ontology summary returned by a paginated list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OntologySummary {
    /// Aggregate identity.
    pub id: OntologyId,
    /// Deployment-wide machine name.
    pub name: String,
    /// Human-facing title.
    pub display_name: String,
    /// Optional human-facing description.
    pub description: Option<String>,
    /// Initial creation time.
    pub created_at: DateTime<Utc>,
    /// Time of the most recent successful replacement.
    pub updated_at: DateTime<Utc>,
}

/// One requested Ontology list page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListOntologies {
    /// One-based page number.
    pub page: u32,
    /// Number of summaries to return.
    pub per_page: u16,
    /// Supported deterministic ordering.
    pub sort: ListSort,
}

impl Default for ListOntologies {
    fn default() -> Self {
        Self {
            page: 1,
            per_page: 20,
            sort: ListSort::UpdatedAtDesc,
        }
    }
}

/// One deterministic Ontology list ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListSort {
    /// Name ascending.
    NameAsc,
    /// Name descending.
    NameDesc,
    /// Display name ascending.
    DisplayNameAsc,
    /// Display name descending.
    DisplayNameDesc,
    /// Creation time ascending.
    CreatedAtAsc,
    /// Creation time descending.
    CreatedAtDesc,
    /// Last update time ascending.
    UpdatedAtAsc,
    /// Last update time descending.
    UpdatedAtDesc,
}

/// A paginated Ontology summary result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OntologyListPage {
    /// Requested page summaries.
    pub data: Vec<OntologySummary>,
    /// One-based page number.
    pub page: u32,
    /// Requested page size.
    pub per_page: u16,
    /// Total matching Ontologies.
    pub total: i64,
}

/// A persisted induced Ontology subgraph.
#[derive(Debug, Clone, PartialEq)]
pub struct Neighborhood {
    /// Object Type used as the traversal origin.
    pub origin_object_type_id: ObjectTypeId,
    /// Maximum bidirectional traversal depth.
    pub depth: u8,
    /// Ordered selected Object Types including their Properties.
    pub object_types: Vec<ObjectType>,
    /// Ordered Link Types whose endpoints are both selected.
    pub link_types: Vec<LinkType>,
    /// Positions associated with selected Object Types.
    pub canvas: Canvas,
}
