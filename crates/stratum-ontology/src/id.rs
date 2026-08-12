//! Strict UUIDv7 identities used by the Ontology aggregate.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use uuid::{Uuid, Variant};

use crate::error::IdParseError;

macro_rules! uuid_v7_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Creates a new UUIDv7 identity.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Returns the underlying UUID.
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

        impl TryFrom<Uuid> for $name {
            type Error = IdParseError;

            fn try_from(value: Uuid) -> Result<Self, Self::Error> {
                if value.get_version_num() == 7 && value.get_variant() == Variant::RFC4122 {
                    Ok(Self(value))
                } else {
                    Err(IdParseError::NotUuidV7)
                }
            }
        }

        impl From<$name> for Uuid {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl FromStr for $name {
            type Err = IdParseError;

            /// Parses a UUIDv7 identity.
            ///
            /// # Errors
            ///
            /// Returns [`IdParseError`] when the input is not a UUIDv7.
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                let uuid = value
                    .parse::<Uuid>()
                    .map_err(|source| IdParseError::InvalidUuid { source })?;
                Self::try_from(uuid)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                value.parse().map_err(de::Error::custom)
            }
        }
    };
}

uuid_v7_id!(OntologyId, "Identity of one Ontology aggregate.");
uuid_v7_id!(ObjectTypeId, "Identity of one Ontology Object Type.");
uuid_v7_id!(PropertyId, "Identity of one Object Type-owned Property.");
uuid_v7_id!(LinkTypeId, "Identity of one Ontology Link Type.");

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{LinkTypeId, ObjectTypeId, OntologyId, PropertyId};

    #[test]
    fn typed_ids_generate_and_parse_only_uuid_v7_values() {
        for value in [
            OntologyId::new().to_string(),
            ObjectTypeId::new().to_string(),
            PropertyId::new().to_string(),
            LinkTypeId::new().to_string(),
        ] {
            assert_eq!(
                value
                    .parse::<uuid::Uuid>()
                    .expect("uuid parses")
                    .get_version_num(),
                7
            );
        }

        assert!(OntologyId::from_str("00000000-0000-4000-8000-000000000000").is_err());
        assert!(
            OntologyId::from_str("00000000-0000-7000-0000-000000000000").is_err(),
            "a version-7 nibble with a non-RFC variant is not a UUIDv7 identity"
        );
        assert!(ObjectTypeId::from_str("not-a-uuid").is_err());
    }
}
