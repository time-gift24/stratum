# stratum-macros conventions

- Keep this crate limited to reusable declarative macros; it is not a procedural-macro crate.
- Generated code must use fully qualified paths so expansion does not depend on imports at the
  invocation site.
- Pass domain-specific types into macros instead of depending on domain crates.
- Add a macro only when it removes real repetition or encodes a shared invariant.
- Verify macro changes through tests in at least one consuming crate.
