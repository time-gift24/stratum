//! Static schema types and structural validation.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{LinkTypeId, ObjectTypeId, OntologyError, PropertyTypeId};

/// One complete ontology schema document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaDocument {
    /// The wire-format version for this schema document.
    pub schema_version: u32,
    /// Ordered object type definitions.
    pub object_types: Vec<ObjectType>,
    /// Ordered link type definitions.
    pub link_types: Vec<LinkType>,
}

impl SchemaDocument {
    /// Validates the schema's static structural invariants.
    ///
    /// # Errors
    ///
    /// Returns [`OntologyError::SchemaInvalid`] when the schema version, names,
    /// identifiers, or link endpoints are invalid.
    pub fn validate(&self) -> Result<(), OntologyError> {
        let mut diagnostics = Vec::new();
        let mut ids = HashSet::new();
        let mut object_type_ids = HashSet::new();
        let mut object_type_names = HashSet::new();
        let mut link_type_names = HashSet::new();

        if self.schema_version != 1 {
            diagnostics.push("schema_version must be 1".to_owned());
        }

        for object_type in &self.object_types {
            validate_id(
                &mut ids,
                object_type.id.as_uuid(),
                "object type",
                &mut diagnostics,
            );
            object_type_ids.insert(object_type.id);
            if !object_type_names.insert(&object_type.name) {
                diagnostics.push(format!("duplicate object type name: {}", object_type.name));
            }

            let mut property_names = HashSet::new();
            for property in &object_type.properties {
                validate_id(
                    &mut ids,
                    property.id.as_uuid(),
                    "property type",
                    &mut diagnostics,
                );
                if !property_names.insert(&property.name) {
                    diagnostics.push(format!(
                        "duplicate property name in object type {}: {}",
                        object_type.name, property.name
                    ));
                }
            }
        }

        for link_type in &self.link_types {
            validate_id(
                &mut ids,
                link_type.id.as_uuid(),
                "link type",
                &mut diagnostics,
            );
            if !link_type_names.insert(&link_type.name) {
                diagnostics.push(format!("duplicate link type name: {}", link_type.name));
            }
            if !object_type_ids.contains(&link_type.source_object_type_id) {
                diagnostics.push(format!(
                    "link type {} has a missing source object type",
                    link_type.name
                ));
            }
            if !object_type_ids.contains(&link_type.target_object_type_id) {
                diagnostics.push(format!(
                    "link type {} has a missing target object type",
                    link_type.name
                ));
            }
        }

        if diagnostics.is_empty() {
            Ok(())
        } else {
            Err(OntologyError::SchemaInvalid { diagnostics })
        }
    }
}

fn validate_id(ids: &mut HashSet<Uuid>, id: Uuid, kind: &str, diagnostics: &mut Vec<String>) {
    if !ids.insert(id) {
        diagnostics.push(format!("duplicate {kind} id: {id}"));
    }
}

/// A named object type and its ordered property definitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectType {
    /// Immutable identity of this object type.
    pub id: ObjectTypeId,
    /// Human-readable object type name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Ordered property definitions for this object type.
    pub properties: Vec<PropertyType>,
}

/// A named, typed value on an [`ObjectType`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyType {
    /// Immutable identity of this property type.
    pub id: PropertyTypeId,
    /// Human-readable property name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Allowed value representation.
    pub value_type: ValueType,
    /// Whether every object must provide this property.
    pub required: bool,
}

/// A directed relationship between two object types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkType {
    /// Immutable identity of this link type.
    pub id: LinkTypeId,
    /// Human-readable link type name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Source object type identity.
    pub source_object_type_id: ObjectTypeId,
    /// Target object type identity.
    pub target_object_type_id: ObjectTypeId,
    /// Allowed endpoint multiplicity.
    pub cardinality: Cardinality,
}

impl LinkType {
    /// Creates a link type with an empty description.
    #[must_use]
    pub fn new(
        id: LinkTypeId,
        name: String,
        source_object_type_id: ObjectTypeId,
        target_object_type_id: ObjectTypeId,
        cardinality: Cardinality,
    ) -> Self {
        Self {
            id,
            name,
            description: String::new(),
            source_object_type_id,
            target_object_type_id,
            cardinality,
        }
    }
}

/// Scalar value representations accepted by properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    /// UTF-8 string.
    String,
    /// Signed integral number.
    Integer,
    /// JSON number.
    Number,
    /// Boolean value.
    Boolean,
    /// RFC 3339 date-time string.
    Datetime,
    /// Arbitrary JSON value.
    Json,
}

/// Endpoint multiplicity for a [`LinkType`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cardinality {
    /// One source may link to one target.
    OneToOne,
    /// One source may link to many targets.
    OneToMany,
    /// Many sources may link to one target.
    ManyToOne,
    /// Many sources may link to many targets.
    ManyToMany,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OntologyError;

    fn person_type_id() -> ObjectTypeId {
        ObjectTypeId::from(Uuid::from_u128(1))
    }

    fn person_type() -> ObjectType {
        ObjectType {
            id: person_type_id(),
            name: "person".to_owned(),
            description: "a person".to_owned(),
            properties: vec![PropertyType {
                id: PropertyTypeId::from(Uuid::from_u128(2)),
                name: "name".to_owned(),
                description: "display name".to_owned(),
                value_type: ValueType::String,
                required: true,
            }],
        }
    }

    #[test]
    fn schema_rejects_a_link_with_a_missing_endpoint_type() {
        let schema = SchemaDocument {
            schema_version: 1,
            object_types: vec![person_type()],
            link_types: vec![LinkType::new(
                LinkTypeId::new(),
                "knows".to_owned(),
                person_type_id(),
                ObjectTypeId::new(),
                Cardinality::ManyToMany,
            )],
        };

        assert!(matches!(
            schema.validate(),
            Err(OntologyError::SchemaInvalid { .. })
        ));
    }

    #[test]
    fn schema_rejects_duplicate_object_type_names() {
        let mut duplicate = person_type();
        duplicate.id = ObjectTypeId::from(Uuid::from_u128(3));

        let schema = SchemaDocument {
            schema_version: 1,
            object_types: vec![person_type(), duplicate],
            link_types: Vec::new(),
        };

        assert!(matches!(
            schema.validate(),
            Err(OntologyError::SchemaInvalid { .. })
        ));
    }

    #[test]
    fn schema_rejects_duplicate_link_type_names() {
        let schema = SchemaDocument {
            schema_version: 1,
            object_types: vec![person_type()],
            link_types: vec![
                LinkType::new(
                    LinkTypeId::from(Uuid::from_u128(3)),
                    "knows".to_owned(),
                    person_type_id(),
                    person_type_id(),
                    Cardinality::ManyToMany,
                ),
                LinkType::new(
                    LinkTypeId::from(Uuid::from_u128(4)),
                    "knows".to_owned(),
                    person_type_id(),
                    person_type_id(),
                    Cardinality::ManyToMany,
                ),
            ],
        };

        assert!(matches!(
            schema.validate(),
            Err(OntologyError::SchemaInvalid { .. })
        ));
    }

    #[test]
    fn schema_rejects_duplicate_property_names_within_an_object_type() {
        let mut object_type = person_type();
        object_type.properties.push(PropertyType {
            id: PropertyTypeId::from(Uuid::from_u128(3)),
            name: "name".to_owned(),
            description: "alternate name".to_owned(),
            value_type: ValueType::String,
            required: false,
        });
        let schema = SchemaDocument {
            schema_version: 1,
            object_types: vec![object_type],
            link_types: Vec::new(),
        };

        assert!(matches!(
            schema.validate(),
            Err(OntologyError::SchemaInvalid { .. })
        ));
    }

    #[test]
    fn schema_accepts_a_valid_document() {
        let schema = SchemaDocument {
            schema_version: 1,
            object_types: vec![person_type()],
            link_types: vec![LinkType::new(
                LinkTypeId::from(Uuid::from_u128(3)),
                "knows".to_owned(),
                person_type_id(),
                person_type_id(),
                Cardinality::ManyToMany,
            )],
        };

        assert_eq!(schema.validate(), Ok(()));
    }

    #[test]
    fn schema_rejects_duplicate_ids_across_schema_resources() {
        let mut schema = SchemaDocument {
            schema_version: 1,
            object_types: vec![person_type()],
            link_types: Vec::new(),
        };
        schema.object_types[0].properties[0].id = PropertyTypeId::from(person_type_id().as_uuid());

        assert!(matches!(
            schema.validate(),
            Err(OntologyError::SchemaInvalid { .. })
        ));
    }

    #[test]
    fn schema_rejects_an_unsupported_schema_version() {
        let schema = SchemaDocument {
            schema_version: 2,
            object_types: vec![person_type()],
            link_types: Vec::new(),
        };

        assert!(matches!(
            schema.validate(),
            Err(OntologyError::SchemaInvalid { .. })
        ));
    }
}
