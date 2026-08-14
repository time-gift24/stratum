//! Deterministic validation for complete Ontology candidates.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{Ontology, OntologyError};

const MAX_OBJECT_TYPES: usize = 500;
const MAX_PROPERTIES_PER_OBJECT_TYPE: usize = 100;
const MAX_TOTAL_PROPERTIES: usize = 10_000;
const MAX_LINK_TYPES: usize = 2_000;
const MAX_CANVAS_POSITIONS: usize = 500;

/// A stable schema violation suitable for API error projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Violation {
    /// Stable machine-readable violation code.
    pub code: ViolationCode,
    /// RFC 6901 JSON Pointer locating the invalid candidate value.
    pub path: String,
    /// Safe human-readable explanation.
    pub message: String,
}

/// Stable Ontology schema violation codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ViolationCode {
    /// The Ontology machine name was invalid.
    InvalidOntologyName,
    /// An Object Type machine name was invalid.
    InvalidObjectTypeName,
    /// A Property machine name was invalid.
    InvalidPropertyName,
    /// A Link Type machine name was invalid.
    InvalidLinkTypeName,
    /// A display name had an invalid Unicode scalar length.
    InvalidDisplayName,
    /// A present description had an invalid Unicode scalar length.
    InvalidDescription,
    /// The Object Type count exceeded its limit.
    TooManyObjectTypes,
    /// An Object Type's Property count exceeded its limit.
    TooManyProperties,
    /// The aggregate Property count exceeded its limit.
    TooManyTotalProperties,
    /// The Link Type count exceeded its limit.
    TooManyLinkTypes,
    /// The canvas position count exceeded its limit.
    TooManyCanvasPositions,
    /// An Object Type ID occurred more than once.
    DuplicateObjectTypeId,
    /// A Property ID occurred more than once.
    DuplicatePropertyId,
    /// A Link Type ID occurred more than once.
    DuplicateLinkTypeId,
    /// An Object Type name occurred more than once.
    DuplicateObjectTypeName,
    /// A Property name occurred more than once under one owner.
    DuplicatePropertyName,
    /// A Link Type name occurred more than once.
    DuplicateLinkTypeName,
    /// A Link Type source endpoint was absent from the document.
    UnknownLinkSourceObjectType,
    /// A Link Type target endpoint was absent from the document.
    UnknownLinkTargetObjectType,
    /// A canvas position occurred more than once for one Object Type.
    DuplicateCanvasPosition,
    /// A canvas position referenced an absent Object Type.
    UnknownCanvasObjectType,
    /// A canvas x coordinate was not finite.
    NonFiniteCanvasX,
    /// A canvas y coordinate was not finite.
    NonFiniteCanvasY,
}

impl ViolationCode {
    /// Returns the stable wire code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidOntologyName => "invalid_ontology_name",
            Self::InvalidObjectTypeName => "invalid_object_type_name",
            Self::InvalidPropertyName => "invalid_property_name",
            Self::InvalidLinkTypeName => "invalid_link_type_name",
            Self::InvalidDisplayName => "invalid_display_name",
            Self::InvalidDescription => "invalid_description",
            Self::TooManyObjectTypes => "too_many_object_types",
            Self::TooManyProperties => "too_many_properties",
            Self::TooManyTotalProperties => "too_many_total_properties",
            Self::TooManyLinkTypes => "too_many_link_types",
            Self::TooManyCanvasPositions => "too_many_canvas_positions",
            Self::DuplicateObjectTypeId => "duplicate_object_type_id",
            Self::DuplicatePropertyId => "duplicate_property_id",
            Self::DuplicateLinkTypeId => "duplicate_link_type_id",
            Self::DuplicateObjectTypeName => "duplicate_object_type_name",
            Self::DuplicatePropertyName => "duplicate_property_name",
            Self::DuplicateLinkTypeName => "duplicate_link_type_name",
            Self::UnknownLinkSourceObjectType => "unknown_link_source_object_type",
            Self::UnknownLinkTargetObjectType => "unknown_link_target_object_type",
            Self::DuplicateCanvasPosition => "duplicate_canvas_position",
            Self::UnknownCanvasObjectType => "unknown_canvas_object_type",
            Self::NonFiniteCanvasX => "non_finite_canvas_x",
            Self::NonFiniteCanvasY => "non_finite_canvas_y",
        }
    }
}

impl Serialize for ViolationCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ViolationCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let code = match value.as_str() {
            "invalid_ontology_name" => Self::InvalidOntologyName,
            "invalid_object_type_name" => Self::InvalidObjectTypeName,
            "invalid_property_name" => Self::InvalidPropertyName,
            "invalid_link_type_name" => Self::InvalidLinkTypeName,
            "invalid_display_name" => Self::InvalidDisplayName,
            "invalid_description" => Self::InvalidDescription,
            "too_many_object_types" => Self::TooManyObjectTypes,
            "too_many_properties" => Self::TooManyProperties,
            "too_many_total_properties" => Self::TooManyTotalProperties,
            "too_many_link_types" => Self::TooManyLinkTypes,
            "too_many_canvas_positions" => Self::TooManyCanvasPositions,
            "duplicate_object_type_id" => Self::DuplicateObjectTypeId,
            "duplicate_property_id" => Self::DuplicatePropertyId,
            "duplicate_link_type_id" => Self::DuplicateLinkTypeId,
            "duplicate_object_type_name" => Self::DuplicateObjectTypeName,
            "duplicate_property_name" => Self::DuplicatePropertyName,
            "duplicate_link_type_name" => Self::DuplicateLinkTypeName,
            "unknown_link_source_object_type" => Self::UnknownLinkSourceObjectType,
            "unknown_link_target_object_type" => Self::UnknownLinkTargetObjectType,
            "duplicate_canvas_position" => Self::DuplicateCanvasPosition,
            "unknown_canvas_object_type" => Self::UnknownCanvasObjectType,
            "non_finite_canvas_x" => Self::NonFiniteCanvasX,
            "non_finite_canvas_y" => Self::NonFiniteCanvasY,
            _ => return Err(serde::de::Error::custom("unknown ontology violation code")),
        };
        Ok(code)
    }
}

/// Validates an Ontology candidate and retains every independently detectable violation.
///
/// # Errors
///
/// Returns [`OntologyError::Validation`] when any schema invariant fails.
pub fn validate(candidate: &Ontology) -> Result<(), OntologyError> {
    let mut violations = Vec::new();

    validate_name(
        &candidate.name,
        ViolationCode::InvalidOntologyName,
        "/name",
        &mut violations,
    );
    validate_text(
        &candidate.display_name,
        "/display_name",
        false,
        &mut violations,
    );
    validate_optional_description(&candidate.description, "/description", &mut violations);

    if candidate.object_types.len() > MAX_OBJECT_TYPES {
        push(
            &mut violations,
            ViolationCode::TooManyObjectTypes,
            "/object_types",
        );
    }
    if candidate.link_types.len() > MAX_LINK_TYPES {
        push(
            &mut violations,
            ViolationCode::TooManyLinkTypes,
            "/link_types",
        );
    }
    if candidate.canvas.positions.len() > MAX_CANVAS_POSITIONS {
        push(
            &mut violations,
            ViolationCode::TooManyCanvasPositions,
            "/canvas/positions",
        );
    }

    let mut object_ids = HashSet::with_capacity(candidate.object_types.len());
    let mut object_names = HashSet::with_capacity(candidate.object_types.len());
    let mut property_ids = HashSet::new();
    let mut total_properties = 0_usize;

    for (object_index, object_type) in candidate.object_types.iter().enumerate() {
        let object_path = format!("/object_types/{object_index}");
        if !object_ids.insert(object_type.id) {
            push(
                &mut violations,
                ViolationCode::DuplicateObjectTypeId,
                &format!("{object_path}/id"),
            );
        }
        if !object_names.insert(object_type.name.as_str()) {
            push(
                &mut violations,
                ViolationCode::DuplicateObjectTypeName,
                &format!("{object_path}/name"),
            );
        }
        validate_name(
            &object_type.name,
            ViolationCode::InvalidObjectTypeName,
            &format!("{object_path}/name"),
            &mut violations,
        );
        validate_text(
            &object_type.display_name,
            &format!("{object_path}/display_name"),
            false,
            &mut violations,
        );
        validate_optional_description(
            &object_type.description,
            &format!("{object_path}/description"),
            &mut violations,
        );
        if object_type.properties.len() > MAX_PROPERTIES_PER_OBJECT_TYPE {
            push(
                &mut violations,
                ViolationCode::TooManyProperties,
                &format!("{object_path}/properties"),
            );
        }
        total_properties = total_properties.saturating_add(object_type.properties.len());

        let mut property_names = HashSet::with_capacity(object_type.properties.len());
        for (property_index, property) in object_type.properties.iter().enumerate() {
            let property_path = format!("{object_path}/properties/{property_index}");
            if !property_ids.insert(property.id) {
                push(
                    &mut violations,
                    ViolationCode::DuplicatePropertyId,
                    &format!("{property_path}/id"),
                );
            }
            if !property_names.insert(property.name.as_str()) {
                push(
                    &mut violations,
                    ViolationCode::DuplicatePropertyName,
                    &format!("{property_path}/name"),
                );
            }
            validate_name(
                &property.name,
                ViolationCode::InvalidPropertyName,
                &format!("{property_path}/name"),
                &mut violations,
            );
            validate_text(
                &property.display_name,
                &format!("{property_path}/display_name"),
                false,
                &mut violations,
            );
            validate_optional_description(
                &property.description,
                &format!("{property_path}/description"),
                &mut violations,
            );
        }
    }

    if total_properties > MAX_TOTAL_PROPERTIES {
        push(
            &mut violations,
            ViolationCode::TooManyTotalProperties,
            "/object_types",
        );
    }

    let mut link_ids = HashSet::with_capacity(candidate.link_types.len());
    let mut link_names = HashSet::with_capacity(candidate.link_types.len());
    for (link_index, link_type) in candidate.link_types.iter().enumerate() {
        let link_path = format!("/link_types/{link_index}");
        if !link_ids.insert(link_type.id) {
            push(
                &mut violations,
                ViolationCode::DuplicateLinkTypeId,
                &format!("{link_path}/id"),
            );
        }
        if !link_names.insert(link_type.name.as_str()) {
            push(
                &mut violations,
                ViolationCode::DuplicateLinkTypeName,
                &format!("{link_path}/name"),
            );
        }
        validate_name(
            &link_type.name,
            ViolationCode::InvalidLinkTypeName,
            &format!("{link_path}/name"),
            &mut violations,
        );
        validate_text(
            &link_type.display_name,
            &format!("{link_path}/display_name"),
            false,
            &mut violations,
        );
        validate_optional_description(
            &link_type.description,
            &format!("{link_path}/description"),
            &mut violations,
        );
        if !object_ids.contains(&link_type.source_object_type_id) {
            push(
                &mut violations,
                ViolationCode::UnknownLinkSourceObjectType,
                &format!("{link_path}/source_object_type_id"),
            );
        }
        if !object_ids.contains(&link_type.target_object_type_id) {
            push(
                &mut violations,
                ViolationCode::UnknownLinkTargetObjectType,
                &format!("{link_path}/target_object_type_id"),
            );
        }
    }

    let mut positioned_ids = HashSet::with_capacity(candidate.canvas.positions.len());
    for (position_index, position) in candidate.canvas.positions.iter().enumerate() {
        let position_path = format!("/canvas/positions/{position_index}");
        if !positioned_ids.insert(position.object_type_id) {
            push(
                &mut violations,
                ViolationCode::DuplicateCanvasPosition,
                &format!("{position_path}/object_type_id"),
            );
        }
        if !object_ids.contains(&position.object_type_id) {
            push(
                &mut violations,
                ViolationCode::UnknownCanvasObjectType,
                &format!("{position_path}/object_type_id"),
            );
        }
        if !position.x.is_finite() {
            push(
                &mut violations,
                ViolationCode::NonFiniteCanvasX,
                &format!("{position_path}/x"),
            );
        }
        if !position.y.is_finite() {
            push(
                &mut violations,
                ViolationCode::NonFiniteCanvasY,
                &format!("{position_path}/y"),
            );
        }
    }

    if violations.is_empty() {
        return Ok(());
    }
    violations.sort_unstable_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.code.as_str().cmp(right.code.as_str()))
    });
    Err(OntologyError::Validation { violations })
}

fn validate_name(value: &str, code: ViolationCode, path: &str, violations: &mut Vec<Violation>) {
    let bytes = value.as_bytes();
    let valid = (1..=64).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes
            .iter()
            .skip(1)
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_');
    if !valid {
        push(violations, code, path);
    }
}

fn validate_text(value: &str, path: &str, description: bool, violations: &mut Vec<Violation>) {
    let length = value.chars().count();
    let valid = if description {
        (1..=2_000).contains(&length)
    } else {
        (1..=200).contains(&length)
    };
    if !valid {
        push(
            violations,
            if description {
                ViolationCode::InvalidDescription
            } else {
                ViolationCode::InvalidDisplayName
            },
            path,
        );
    }
}

fn validate_optional_description(
    value: &Option<String>,
    path: &str,
    violations: &mut Vec<Violation>,
) {
    if let Some(value) = value {
        validate_text(value, path, true, violations);
    }
}

fn push(violations: &mut Vec<Violation>, code: ViolationCode, path: &str) {
    violations.push(Violation {
        code,
        path: path.to_owned(),
        message: message(code).to_owned(),
    });
}

const fn message(code: ViolationCode) -> &'static str {
    match code {
        ViolationCode::InvalidOntologyName
        | ViolationCode::InvalidObjectTypeName
        | ViolationCode::InvalidPropertyName
        | ViolationCode::InvalidLinkTypeName => "name must match the required pattern",
        ViolationCode::InvalidDisplayName => "display name has an invalid length",
        ViolationCode::InvalidDescription => "description has an invalid length",
        ViolationCode::TooManyObjectTypes => "too many object types",
        ViolationCode::TooManyProperties => "too many properties",
        ViolationCode::TooManyTotalProperties => "too many total properties",
        ViolationCode::TooManyLinkTypes => "too many link types",
        ViolationCode::TooManyCanvasPositions => "too many canvas positions",
        ViolationCode::DuplicateObjectTypeId
        | ViolationCode::DuplicatePropertyId
        | ViolationCode::DuplicateLinkTypeId => "id must be unique",
        ViolationCode::DuplicateObjectTypeName
        | ViolationCode::DuplicatePropertyName
        | ViolationCode::DuplicateLinkTypeName => "name must be unique in its scope",
        ViolationCode::UnknownLinkSourceObjectType => "link source object type is unknown",
        ViolationCode::UnknownLinkTargetObjectType => "link target object type is unknown",
        ViolationCode::DuplicateCanvasPosition => "object type has more than one canvas position",
        ViolationCode::UnknownCanvasObjectType => "canvas object type is unknown",
        ViolationCode::NonFiniteCanvasX => "canvas x coordinate must be finite",
        ViolationCode::NonFiniteCanvasY => "canvas y coordinate must be finite",
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Canvas, CanvasPosition, Cardinality, LinkType, ObjectType, ObjectTypeId, Ontology,
        OntologyError, OntologyId, Property, PropertyId, ValueType,
    };

    use super::{ViolationCode, validate};

    fn empty_ontology() -> Ontology {
        Ontology {
            id: OntologyId::new(),
            name: "ontology".to_owned(),
            display_name: "Ontology".to_owned(),
            description: None,
            object_types: Vec::new(),
            link_types: Vec::new(),
            canvas: Canvas {
                positions: Vec::new(),
            },
        }
    }

    fn object_type(name: &str, property_name: &str) -> ObjectType {
        ObjectType {
            id: ObjectTypeId::new(),
            name: name.to_owned(),
            display_name: name.to_owned(),
            description: None,
            properties: vec![Property {
                id: PropertyId::new(),
                name: property_name.to_owned(),
                display_name: property_name.to_owned(),
                description: None,
                value_type: ValueType::String,
                required: false,
            }],
        }
    }

    fn object_type_without_properties(index: usize) -> ObjectType {
        ObjectType {
            id: ObjectTypeId::new(),
            name: format!("type_{index}"),
            display_name: format!("Type {index}"),
            description: None,
            properties: Vec::new(),
        }
    }

    fn property(index: usize) -> Property {
        Property {
            id: PropertyId::new(),
            name: format!("property_{index}"),
            display_name: format!("Property {index}"),
            description: None,
            value_type: ValueType::String,
            required: false,
        }
    }

    fn has_code(candidate: &Ontology, code: ViolationCode) -> bool {
        let Err(OntologyError::Validation { violations }) = validate(candidate) else {
            return false;
        };
        violations.iter().any(|violation| violation.code == code)
    }

    #[test]
    fn accepts_empty_schema_and_property_name_reuse_across_owners() {
        let mut candidate = empty_ontology();
        candidate.object_types = vec![
            object_type("person", "name"),
            object_type("company", "name"),
        ];

        assert!(validate(&empty_ontology()).is_ok());
        assert!(validate(&candidate).is_ok());
    }

    #[test]
    fn reports_duplicate_scopes_and_dangling_references_in_deterministic_order() {
        let mut candidate = empty_ontology();
        let duplicate_id = ObjectTypeId::new();
        candidate.object_types = vec![
            ObjectType {
                id: duplicate_id,
                name: "thing".to_owned(),
                display_name: String::new(),
                description: Some(String::new()),
                properties: vec![
                    Property {
                        id: PropertyId::new(),
                        name: "name".to_owned(),
                        display_name: "Name".to_owned(),
                        description: None,
                        value_type: ValueType::DateTime,
                        required: false,
                    },
                    Property {
                        id: PropertyId::new(),
                        name: "name".to_owned(),
                        display_name: "Name".to_owned(),
                        description: None,
                        value_type: ValueType::Boolean,
                        required: false,
                    },
                ],
            },
            ObjectType {
                id: duplicate_id,
                name: "thing".to_owned(),
                display_name: "Thing".to_owned(),
                description: None,
                properties: Vec::new(),
            },
        ];
        candidate.link_types.push(LinkType {
            id: crate::LinkTypeId::new(),
            name: "relates_to".to_owned(),
            display_name: "Relates to".to_owned(),
            description: None,
            source_object_type_id: ObjectTypeId::new(),
            target_object_type_id: ObjectTypeId::new(),
            source_to_target: Cardinality::Many,
            target_to_source: Cardinality::One,
        });
        candidate.canvas.positions.push(CanvasPosition {
            object_type_id: ObjectTypeId::new(),
            x: f64::NAN,
            y: f64::INFINITY,
        });

        let Err(OntologyError::Validation { violations }) = validate(&candidate) else {
            panic!("candidate should be invalid");
        };
        let paths = violations
            .iter()
            .map(|violation| violation.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(paths, {
            let mut expected = paths.clone();
            expected.sort_unstable();
            expected
        });
        assert!(
            violations
                .iter()
                .any(|violation| violation.code == ViolationCode::DuplicateObjectTypeId)
        );
        assert!(
            violations
                .iter()
                .any(|violation| violation.code == ViolationCode::DuplicatePropertyName)
        );
        assert!(
            violations
                .iter()
                .any(|violation| violation.code == ViolationCode::UnknownLinkSourceObjectType)
        );
        assert!(
            violations
                .iter()
                .any(|violation| violation.code == ViolationCode::UnknownCanvasObjectType)
        );
    }

    #[test]
    fn enforces_unicode_scalar_boundaries() {
        let mut candidate = empty_ontology();
        candidate.display_name = "é".repeat(200);
        candidate.description = Some("界".repeat(2_000));
        assert!(validate(&candidate).is_ok());

        candidate.display_name.push('x');
        candidate.description = Some("界".repeat(2_001));
        assert!(matches!(
            validate(&candidate),
            Err(OntologyError::Validation { .. })
        ));
    }

    #[test]
    fn accepts_every_scalar_enum_and_cardinality_value() {
        for value_type in [
            ValueType::String,
            ValueType::Integer,
            ValueType::Number,
            ValueType::Boolean,
            ValueType::Date,
            ValueType::DateTime,
        ] {
            let encoded = serde_json::to_string(&value_type).expect("value type encodes");
            let decoded = serde_json::from_str::<ValueType>(&encoded).expect("value type decodes");
            assert_eq!(decoded, value_type);
        }
        for cardinality in [Cardinality::One, Cardinality::Many] {
            let encoded = serde_json::to_string(&cardinality).expect("cardinality encodes");
            let decoded =
                serde_json::from_str::<Cardinality>(&encoded).expect("cardinality decodes");
            assert_eq!(decoded, cardinality);
        }
    }

    #[test]
    fn enforces_every_collection_limit_at_its_boundary() {
        let mut object_limit = empty_ontology();
        object_limit.object_types = (0..500).map(object_type_without_properties).collect();
        assert!(validate(&object_limit).is_ok());
        object_limit
            .object_types
            .push(object_type_without_properties(500));
        assert!(has_code(&object_limit, ViolationCode::TooManyObjectTypes));

        let mut per_owner_limit = empty_ontology();
        let mut owner = object_type_without_properties(0);
        owner.properties = (0..100).map(property).collect();
        per_owner_limit.object_types = vec![owner.clone()];
        assert!(validate(&per_owner_limit).is_ok());
        owner.properties.push(property(100));
        per_owner_limit.object_types = vec![owner];
        assert!(has_code(&per_owner_limit, ViolationCode::TooManyProperties));

        let mut total_limit = empty_ontology();
        total_limit.object_types = (0..100)
            .map(|object_index| {
                let mut object_type = object_type_without_properties(object_index);
                object_type.properties = (0..100)
                    .map(|property_index| property(object_index * 100 + property_index))
                    .collect();
                object_type
            })
            .collect();
        assert!(validate(&total_limit).is_ok());
        let mut extra = object_type_without_properties(100);
        extra.properties.push(property(10_000));
        total_limit.object_types.push(extra);
        assert!(has_code(
            &total_limit,
            ViolationCode::TooManyTotalProperties
        ));

        let mut link_limit = empty_ontology();
        let endpoint = object_type_without_properties(0);
        link_limit.object_types = vec![endpoint.clone()];
        link_limit.link_types = (0..2_000)
            .map(|index| LinkType {
                id: crate::LinkTypeId::new(),
                name: format!("link_{index}"),
                display_name: format!("Link {index}"),
                description: None,
                source_object_type_id: endpoint.id,
                target_object_type_id: endpoint.id,
                source_to_target: Cardinality::One,
                target_to_source: Cardinality::Many,
            })
            .collect();
        assert!(validate(&link_limit).is_ok());
        link_limit.link_types.push(LinkType {
            id: crate::LinkTypeId::new(),
            name: "link_2000".to_owned(),
            display_name: "Link 2000".to_owned(),
            description: None,
            source_object_type_id: endpoint.id,
            target_object_type_id: endpoint.id,
            source_to_target: Cardinality::One,
            target_to_source: Cardinality::Many,
        });
        assert!(has_code(&link_limit, ViolationCode::TooManyLinkTypes));

        let mut canvas_limit = empty_ontology();
        canvas_limit.object_types = (0..500).map(object_type_without_properties).collect();
        canvas_limit.canvas.positions = canvas_limit
            .object_types
            .iter()
            .map(|object_type| CanvasPosition {
                object_type_id: object_type.id,
                x: 0.0,
                y: 0.0,
            })
            .collect();
        assert!(validate(&canvas_limit).is_ok());
        canvas_limit.canvas.positions.push(CanvasPosition {
            object_type_id: canvas_limit.object_types[0].id,
            x: 1.0,
            y: 1.0,
        });
        assert!(has_code(
            &canvas_limit,
            ViolationCode::TooManyCanvasPositions
        ));
    }

    #[test]
    fn sorts_violations_by_path_then_stable_code() {
        let mut candidate = empty_ontology();
        candidate.object_types = vec![
            ObjectType {
                id: ObjectTypeId::new(),
                name: "bad-name".to_owned(),
                display_name: "Object".to_owned(),
                description: None,
                properties: Vec::new(),
            },
            ObjectType {
                id: ObjectTypeId::new(),
                name: "bad-name".to_owned(),
                display_name: "Object".to_owned(),
                description: None,
                properties: Vec::new(),
            },
        ];

        let Err(OntologyError::Validation { violations }) = validate(&candidate) else {
            panic!("candidate should be invalid");
        };
        let second_name_codes = violations
            .iter()
            .filter(|violation| violation.path == "/object_types/1/name")
            .map(|violation| violation.code.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            second_name_codes,
            vec!["duplicate_object_type_name", "invalid_object_type_name"]
        );
    }
}
