use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use wyse_filesystem::{
    CasExpectation, DirEntry, Entry, FileMetadata, Filesystem, FilesystemError, RecordVersion,
    VersionedEntry, VirtualPath,
};

#[derive(Default)]
pub(super) struct MemoryCasFilesystem {
    records: Mutex<BTreeMap<VirtualPath, VersionedEntry>>,
    directories: Mutex<BTreeSet<VirtualPath>>,
    next_version: AtomicU64,
    fail_next_version_write: AtomicBool,
}

impl MemoryCasFilesystem {
    pub(super) fn exists(&self, path: &str) -> bool {
        let path = VirtualPath::try_from(path).expect("valid fixture path");
        self.records
            .lock()
            .expect("records mutex")
            .contains_key(&path)
    }

    pub(super) fn insert_entry(&self, path: &str, entry: Entry) {
        let path = VirtualPath::try_from(path).expect("valid fixture path");
        let version = self.next_record_version();
        self.records
            .lock()
            .expect("records mutex")
            .insert(path, VersionedEntry { entry, version });
    }

    pub(super) fn remove_entry(&self, path: &str) {
        let path = VirtualPath::try_from(path).expect("valid fixture path");
        self.records.lock().expect("records mutex").remove(&path);
    }

    pub(super) fn entry_version(&self, path: &str) -> Option<RecordVersion> {
        let path = VirtualPath::try_from(path).expect("valid fixture path");
        self.records
            .lock()
            .expect("records mutex")
            .get(&path)
            .map(|record| record.version)
    }

    pub(super) fn fail_next_version_write(&self) {
        self.fail_next_version_write.store(true, Ordering::SeqCst);
    }

    pub(super) fn version_write_failure_pending(&self) -> bool {
        self.fail_next_version_write.load(Ordering::SeqCst)
    }

    fn next_record_version(&self) -> RecordVersion {
        RecordVersion::from_backend(self.next_version.fetch_add(1, Ordering::SeqCst))
    }
}

#[async_trait]
impl Filesystem for MemoryCasFilesystem {
    async fn get(&self, path: &VirtualPath) -> Result<Option<VersionedEntry>, FilesystemError> {
        Ok(self
            .records
            .lock()
            .expect("records mutex")
            .get(path)
            .cloned())
    }

    async fn put(
        &self,
        path: &VirtualPath,
        entry: Entry,
        cas: CasExpectation,
    ) -> Result<RecordVersion, FilesystemError> {
        let mut records = self.records.lock().expect("records mutex");
        if matches!(cas, CasExpectation::Version(_))
            && self.fail_next_version_write.swap(false, Ordering::SeqCst)
        {
            return Err(FilesystemError::VersionMismatch { path: path.clone() });
        }

        match cas {
            CasExpectation::Absent if records.contains_key(path) => {
                Err(FilesystemError::VersionMismatch { path: path.clone() })
            }
            CasExpectation::Version(expected)
                if records.get(path).map(|record| record.version) != Some(expected) =>
            {
                Err(FilesystemError::VersionMismatch { path: path.clone() })
            }
            CasExpectation::Absent | CasExpectation::Version(_) | CasExpectation::Any => {
                let version = self.next_record_version();
                records.insert(path.clone(), VersionedEntry { entry, version });
                Ok(version)
            }
        }
    }

    async fn read_file(&self, path: &VirtualPath) -> Result<Vec<u8>, FilesystemError> {
        self.get(path)
            .await?
            .map(|record| record.entry.into_contents())
            .ok_or_else(|| FilesystemError::NotFound { path: path.clone() })
    }

    async fn write_file(
        &self,
        path: &VirtualPath,
        contents: Vec<u8>,
    ) -> Result<(), FilesystemError> {
        self.put(path, Entry::new(contents), CasExpectation::Any)
            .await
            .map(|_| ())
    }

    async fn list_dir(&self, path: &VirtualPath) -> Result<Vec<DirEntry>, FilesystemError> {
        if self
            .directories
            .lock()
            .expect("directories mutex")
            .contains(path)
        {
            Ok(Vec::new())
        } else {
            Err(FilesystemError::NotFound { path: path.clone() })
        }
    }

    async fn metadata(&self, path: &VirtualPath) -> Result<FileMetadata, FilesystemError> {
        Err(FilesystemError::NotFound { path: path.clone() })
    }

    async fn create_dir(&self, path: &VirtualPath) -> Result<(), FilesystemError> {
        self.directories
            .lock()
            .expect("directories mutex")
            .insert(path.clone());
        Ok(())
    }

    async fn remove_file(&self, path: &VirtualPath) -> Result<(), FilesystemError> {
        self.records
            .lock()
            .expect("records mutex")
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| FilesystemError::NotFound { path: path.clone() })
    }

    async fn remove_dir(&self, path: &VirtualPath) -> Result<(), FilesystemError> {
        self.directories
            .lock()
            .expect("directories mutex")
            .remove(path)
            .then_some(())
            .ok_or_else(|| FilesystemError::NotFound { path: path.clone() })
    }
}
