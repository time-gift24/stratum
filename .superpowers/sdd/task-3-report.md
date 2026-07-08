# Task 3 Report: Local Sandbox Backend

## What changed

- Added `crates/wyse-filesystem/src/local.rs` with `LocalFilesystem` and `LocalFilesystemConfig`.
- Implemented sandboxed local filesystem operations: `read_file`, `write_file`, `list_dir`, `metadata`, `create_dir`, `remove_file`, and `remove_dir`.
- Enforced sandbox boundaries by canonicalizing the configured root and resolving host paths only inside that root.
- Enforced `max_file_bytes` for whole-file reads and writes.
- Exported the backend from `crates/wyse-filesystem/src/lib.rs`.
- Preserved the existing `FilesystemError` model and did not add any `apply_patch`-related filesystem API.

## Tests and outputs

### RED

Command:

```bash
cargo test -p wyse-filesystem local::tests
```

Initial expected failure:

- The crate failed to compile because `LocalFilesystem` and `LocalFilesystemConfig` were not implemented yet.

Relevant compiler output:

```text
error[E0432]: unresolved imports `local::LocalFilesystem`, `local::LocalFilesystemConfig`
error[E0422]: cannot find struct, variant or union type `LocalFilesystemConfig`
error[E0433]: cannot find type `LocalFilesystem` in this scope
```

### GREEN

Command:

```bash
cargo test -p wyse-filesystem local::tests
cargo fmt
cargo test -p wyse-filesystem local::tests
```

Final test output:

```text
running 3 tests
test local::tests::rejects_reads_larger_than_limit ... ok
test local::tests::remove_dir_rejects_non_empty_directory ... ok
test local::tests::reads_writes_lists_and_removes_inside_sandbox ... ok

test result: ok. 3 passed; 0 failed
```

## TDD evidence

- Wrote the local backend tests first, before implementation.
- Verified the crate failed for the missing backend types.
- Implemented the backend minimally to satisfy those tests.
- Re-ran the focused test suite and confirmed all three tests passed.

## Files changed

- `crates/wyse-filesystem/src/local.rs`
- `crates/wyse-filesystem/src/lib.rs`

## Self-review

- The backend stays small and focused on the existing filesystem trait.
- Sandbox escape prevention is handled by canonicalizing real host paths before access.
- The implementation preserves the current error taxonomy instead of introducing new filesystem-specific wrappers.
- The task scope was kept tight: no patch API, no extra docs work, and no broader filesystem refactor.

## Concerns

- The current tests cover the happy path, non-empty directory removal, and read-size limits, but they do not yet exercise symlink edge cases or explicit sandbox-escape attempts.
- `crates/wyse-filesystem/src/error.rs` and `crates/wyse-filesystem/src/definition.rs` did not need functional changes for this task because the existing types already covered the backend.

## Fix report

- Hardened `write_file` so an existing target is canonicalized and rejected if it resolves outside the sandbox; non-existent targets still use the parent-directory check.
- Switched path classification to symlink-aware inspection so symlinks stay visible as `FileType::Symlink` in `metadata()` and `list_dir()`.
- Replaced the `child_virtual_path()` panic with a typed `FilesystemError::InvalidVirtualPath`.
- Added focused tests for symlink reporting, symlink escape rejection, and invalid child entry names.
- Verified with `cargo test -p wyse-filesystem local::tests` and `cargo fmt`.

## Task 3 follow-up fix

- Special-cased the sandbox root in parent validation so `metadata("/")` resolves against the root directory itself instead of the host parent directory.
- Kept `create_dir("/")` on the normal filesystem path, which now returns an existing-path error from the OS instead of `PathEscapesSandbox`.
- Hardened `write_file` against dangling final-component symlinks by checking `symlink_metadata()` on the destination path before falling back to parent validation.
- Added focused regression tests for root metadata, root `create_dir`, and a Unix dangling-symlink write escape that must reject the write and leave the outside target untouched.
