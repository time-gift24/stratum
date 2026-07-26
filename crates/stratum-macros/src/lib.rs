//! Declarative macros shared across Stratum crates.

/// Defines a UUIDv7-backed identity newtype with standard conversions.
#[macro_export]
macro_rules! uuid_identity {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(::uuid::Uuid);

        impl $name {
            /// Creates a new UUIDv7 identity.
            #[must_use]
            pub fn new() -> Self {
                Self(::uuid::Uuid::now_v7())
            }

            /// Returns the inner UUID.
            #[must_use]
            pub const fn as_uuid(self) -> ::uuid::Uuid {
                self.0
            }
        }

        impl ::std::default::Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl ::std::convert::From<::uuid::Uuid> for $name {
            fn from(value: ::uuid::Uuid) -> Self {
                Self(value)
            }
        }

        impl ::std::convert::From<$name> for ::uuid::Uuid {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = ::uuid::Error;

            fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
                value.parse::<::uuid::Uuid>().map(Self)
            }
        }
    };
}

/// Defines a string-backed identity newtype with standard conversions.
#[macro_export]
macro_rules! string_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(::std::string::String);

        impl $name {
            /// Creates a new id from a string-like value.
            #[must_use]
            pub fn new(value: impl ::std::convert::Into<::std::string::String>) -> Self {
                Self(value.into())
            }

            /// Returns the id as a string slice.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl ::std::convert::From<::std::string::String> for $name {
            fn from(value: ::std::string::String) -> Self {
                Self(value)
            }
        }

        impl ::std::convert::From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }
    };
}

/// Defines a validated lowercase SHA-256 hexadecimal fingerprint newtype.
#[macro_export]
macro_rules! sha256_fingerprint {
    ($name:ident, $doc:literal, $error:path) => {
        #[doc = $doc]
        #[derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(::std::string::String);

        impl $name {
            /// Returns the canonical lowercase hexadecimal digest.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $error;

            fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
                if value.len() == 64
                    && value
                        .as_bytes()
                        .iter()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
                {
                    Ok(Self(value.to_owned()))
                } else {
                    Err(<$error>::InvalidFormat)
                }
            }
        }

        impl ::std::convert::TryFrom<::std::string::String> for $name {
            type Error = $error;

            fn try_from(value: ::std::string::String) -> ::std::result::Result<Self, Self::Error> {
                value.parse()
            }
        }

        impl ::std::convert::From<$name> for ::std::string::String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}
