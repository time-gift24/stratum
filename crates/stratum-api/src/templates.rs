//! Read-only hot template catalog over `[agent].templates_root`.
//!
//! The catalog is read through a sandboxed [`LocalFilesystem`] used strictly
//! read-only: every request reads the current files, any unreadable or
//! invalid template fails the whole catalog (no partial results), and only
//! safe fields (name plus the provider default model configuration) leave
//! the API. The service never creates template directories.

use std::path::Path;

use stratum_config::{AgentName, Config, ConfigError, ResolvedAgentDefinition};
use stratum_filesystem::{
    FileType, Filesystem, FilesystemError, LocalFilesystem, LocalFilesystemConfig, VirtualPath,
};
use stratum_llm::LlmProviderManager;

use crate::dto::AgentTemplateDto;
use crate::error::{ApiError, ErrorKind};

/// Sandboxed read-only view of the template catalog.
#[derive(Debug)]
pub(crate) struct TemplateCatalog {
    fs: LocalFilesystem,
    config: Config,
}

impl TemplateCatalog {
    /// Opens the catalog root and proves it exists, is a directory, and is
    /// readable. An empty catalog is valid; the directory is never created.
    ///
    /// # Errors
    ///
    /// Returns the filesystem failure when the root is missing, not a
    /// directory, or cannot be listed; the caller fails startup.
    pub(crate) async fn new(root: &Path, config: Config) -> Result<Self, FilesystemError> {
        let fs = LocalFilesystem::new(LocalFilesystemConfig {
            root: root.to_path_buf(),
            max_file_bytes: None,
        })?;
        let catalog = Self { fs, config };
        catalog.fs.list_dir(&root_path_sandbox()).await?;
        Ok(catalog)
    }

    /// Hot-reads and resolves one template by Agent name.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::TemplateNotFound`] when no `*.toml` file exists
    /// for the name, [`ErrorKind::InvalidAgentTemplate`] when the file cannot
    /// be read, parsed, or validated, and [`ErrorKind::ModelNotConfigured`]
    /// when its model is not configured.
    pub(crate) async fn resolve(
        &self,
        agent_name: &AgentName,
    ) -> Result<ResolvedAgentDefinition, ApiError> {
        let path = template_path(agent_name.as_str())?;
        let bytes = self
            .fs
            .read_file(&path)
            .await
            .map_err(|error| match error {
                FilesystemError::NotFound { .. } => ApiError::new(ErrorKind::TemplateNotFound),
                other => ApiError::with_source(ErrorKind::InvalidAgentTemplate, other),
            })?;
        let text = String::from_utf8(bytes)
            .map_err(|source| ApiError::with_source(ErrorKind::InvalidAgentTemplate, source))?;
        self.config
            .resolve_template(agent_name.clone(), &text)
            .map_err(map_config_error)
    }

    /// Reads the full catalog, all-or-nothing.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::InvalidAgentTemplate`] when any entry cannot be
    /// listed, read, parsed, or validated, and
    /// [`ErrorKind::ModelNotConfigured`] when a template model has no
    /// registered provider defaults.
    pub(crate) async fn list(
        &self,
        providers: &LlmProviderManager,
    ) -> Result<Vec<AgentTemplateDto>, ApiError> {
        let entries = self
            .fs
            .list_dir(&root_path()?)
            .await
            .map_err(|source| ApiError::with_source(ErrorKind::InvalidAgentTemplate, source))?;
        let mut templates = Vec::new();
        for entry in entries {
            if entry.file_type != FileType::File || !entry.file_name.ends_with(".toml") {
                continue;
            }
            let stem = entry.file_name.trim_end_matches(".toml");
            let agent_name: AgentName = stem
                .parse()
                .map_err(|source| ApiError::with_source(ErrorKind::InvalidAgentTemplate, source))?;
            let definition = self.resolve(&agent_name).await?;
            let model_config = providers
                .default_model_config(&definition.model)
                .map_err(|_| ApiError::new(ErrorKind::ModelNotConfigured))?;
            templates.push(AgentTemplateDto {
                agent_name: agent_name.as_str().to_owned(),
                model_config,
            });
        }
        templates.sort_by(|left, right| left.agent_name.cmp(&right.agent_name));
        Ok(templates)
    }
}

/// Maps a template validation failure; `ModelNotConfigured` keeps its own
/// stable code.
fn map_config_error(source: ConfigError) -> ApiError {
    match source {
        ConfigError::ModelNotConfigured { .. } => {
            ApiError::with_source(ErrorKind::ModelNotConfigured, source)
        }
        other => ApiError::with_source(ErrorKind::InvalidAgentTemplate, other),
    }
}

fn root_path() -> Result<VirtualPath, ApiError> {
    VirtualPath::try_from("/").map_err(|_| ApiError::new(ErrorKind::Internal))
}

fn root_path_sandbox() -> VirtualPath {
    // Invariant: "/" is a constant valid virtual path.
    VirtualPath::try_from("/").expect("the sandbox root path is always valid")
}

fn template_path(agent_name: &str) -> Result<VirtualPath, ApiError> {
    let path = format!("/{agent_name}.toml");
    VirtualPath::try_from(path.as_str()).map_err(|_| ApiError::new(ErrorKind::InvalidAgentTemplate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_paths_are_sandboxed_virtual_paths() {
        let path = template_path("coding-agent").expect("path is valid");
        assert_eq!(path.as_str(), "/coding-agent.toml");
    }
}
