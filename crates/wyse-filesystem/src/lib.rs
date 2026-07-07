//! Virtual filesystem abstractions and local sandbox backend for Wyse agents.

pub mod definition;
pub mod error;
pub mod patch;
pub mod path;

pub use definition::{DirEntry, FileMetadata, FileType, Filesystem};
pub use error::FilesystemError;
pub use patch::{Patch, PatchApplyReport};
pub use path::{VirtualPath, VirtualPathError};
