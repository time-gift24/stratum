//! Strongly typed identifiers used by the ontology domain.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::OntologyError;

macro_rules! uuid_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Creates a new UUIDv7 identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Returns the wrapped UUID.
            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl From<Uuid> for $name {
            fn from(value: Uuid) -> Self {
                Self(value)
            }
        }

        impl From<$name> for Uuid {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            /// Parses a UUID identifier.
            ///
            /// # Errors
            ///
            /// Returns [`uuid::Error`] when `value` is not a valid UUID.
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                value.parse::<Uuid>().map(Self)
            }
        }
    };
}

uuid_id!(ObjectTypeId, "Identity of an object type.");
uuid_id!(PropertyTypeId, "Identity of a property type.");
uuid_id!(LinkTypeId, "Identity of a link type.");
uuid_id!(ObjectId, "Identity of an object instance.");
uuid_id!(LinkId, "Identity of a link instance.");

/// Validated logical name of an editable schema draft.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DraftName(String);

impl DraftName {
    /// Returns the name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DraftName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl TryFrom<String> for DraftName {
    type Error = OntologyError;

    /// Validates a logical draft name.
    ///
    /// # Errors
    ///
    /// Returns [`OntologyError::InvalidDraftName`] when the value is empty, longer
    /// than 64 bytes, starts with a non-ASCII letter or digit, or contains a byte
    /// other than an ASCII letter, digit, underscore, or hyphen.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        valid_name(&value)
            .then_some(Self(value))
            .ok_or(OntologyError::InvalidDraftName)
    }
}

impl FromStr for DraftName {
    type Err = OntologyError;

    /// Parses and validates a logical draft name.
    ///
    /// # Errors
    ///
    /// Returns [`OntologyError::InvalidDraftName`] when the value is empty, longer
    /// than 64 bytes, starts with a non-ASCII letter or digit, or contains a byte
    /// other than an ASCII letter, digit, underscore, or hyphen.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl From<DraftName> for String {
    fn from(value: DraftName) -> Self {
        value.0
    }
}

/// Validated logical name of a schema tag.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TagName(String);

impl TagName {
    /// Returns the reserved tag used by the running service.
    #[must_use]
    pub fn online() -> Self {
        Self("online".to_owned())
    }

    /// Returns the name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TagName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl TryFrom<String> for TagName {
    type Error = OntologyError;

    /// Validates a logical tag name.
    ///
    /// # Errors
    ///
    /// Returns [`OntologyError::InvalidTagName`] when the value is empty, longer
    /// than 64 bytes, starts with a non-ASCII letter or digit, or contains a byte
    /// other than an ASCII letter, digit, underscore, or hyphen.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        valid_name(&value)
            .then_some(Self(value))
            .ok_or(OntologyError::InvalidTagName)
    }
}

impl FromStr for TagName {
    type Err = OntologyError;

    /// Parses and validates a logical tag name.
    ///
    /// # Errors
    ///
    /// Returns [`OntologyError::InvalidTagName`] when the value is empty, longer
    /// than 64 bytes, starts with a non-ASCII letter or digit, or contains a byte
    /// other than an ASCII letter, digit, underscore, or hyphen.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl From<TagName> for String {
    fn from(value: TagName) -> Self {
        value.0
    }
}

/// Content-addressed identity of one published schema revision.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RevisionId(String);

impl RevisionId {
    /// Returns the hexadecimal digest as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RevisionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl TryFrom<String> for RevisionId {
    type Error = OntologyError;

    /// Validates a content-addressed schema revision id.
    ///
    /// # Errors
    ///
    /// Returns [`OntologyError::InvalidRevisionId`] when the value is not exactly
    /// 64 lowercase ASCII hexadecimal digits.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        (value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')))
        .then_some(Self(value))
        .ok_or(OntologyError::InvalidRevisionId)
    }
}

impl FromStr for RevisionId {
    type Err = OntologyError;

    /// Parses and validates a content-addressed schema revision id.
    ///
    /// # Errors
    ///
    /// Returns [`OntologyError::InvalidRevisionId`] when the value is not exactly
    /// 64 lowercase ASCII hexadecimal digits.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl From<RevisionId> for String {
    fn from(value: RevisionId) -> Self {
        value.0
    }
}

/// A caller-selected schema resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaRef {
    /// An editable draft.
    Draft(DraftName),
    /// An immutable published revision.
    Revision(RevisionId),
    /// A mutable tag pointing to a revision.
    Tag(TagName),
}

fn valid_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=64).contains(&bytes.len())
        && matches!(bytes.first(), Some(byte) if byte.is_ascii_alphanumeric())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_name_accepts_the_documented_format() {
        let name = DraftName::try_from("draft_1-name".to_owned()).expect("valid draft name");

        assert_eq!(name.as_str(), "draft_1-name");
    }

    #[test]
    fn draft_name_rejects_a_path_separator() {
        assert!(matches!(
            DraftName::try_from("draft/name".to_owned()),
            Err(OntologyError::InvalidDraftName)
        ));
    }

    #[test]
    fn revision_id_rejects_an_uppercase_hexadecimal_digit() {
        let revision = format!("{}A", "a".repeat(63));

        assert!(matches!(
            RevisionId::try_from(revision),
            Err(OntologyError::InvalidRevisionId)
        ));
    }

    #[test]
    fn object_type_ids_round_trip_through_uuid() {
        let uuid = Uuid::from_u128(1);
        let id = ObjectTypeId::from(uuid);

        assert_eq!(Uuid::from(id), uuid);
    }
}
