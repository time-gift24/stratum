use std::sync::Arc;

use stratum_filesystem::{LocalFilesystem, LocalFilesystemConfig};
use stratum_ontology::{DraftName, FilesystemDraftStore, SchemaDocument};

#[tokio::test]
async fn fresh_local_filesystem_is_a_usable_empty_draft_store()
-> Result<(), Box<dyn std::error::Error>> {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "stratum-ontology-drafts-{}-{unique}",
        std::process::id()
    ));
    tokio::fs::create_dir_all(&root).await?;
    let filesystem = Arc::new(LocalFilesystem::new(LocalFilesystemConfig {
        root: root.clone(),
        max_file_bytes: None,
    })?);
    let store = FilesystemDraftStore::new(filesystem);

    assert!(store.list().await?.is_empty());
    let created = store
        .create(
            DraftName::try_from("main".to_owned())?,
            SchemaDocument {
                schema_version: 1,
                object_types: Vec::new(),
                link_types: Vec::new(),
            },
        )
        .await?;
    assert_eq!(store.list().await?, vec![created]);

    tokio::fs::remove_dir_all(root).await?;
    Ok(())
}
