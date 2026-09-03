## 17. Upstream references

These official sources inform the standards where project rules are silent. They are guidance, not
an unversioned way to change the repository's contract:

- [Rust Style Guide](https://doc.rust-lang.org/style-guide/) — formatting principles; rustfmt is
  its executable implementation.
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/checklist.html) — naming,
  interoperability, documentation, predictability and API evolution.
- [Clippy documentation](https://doc.rust-lang.org/stable/clippy/) and
  [lint-group policy](https://doc.rust-lang.org/stable/clippy/usage.html) — why allow-by-default
  groups are selected deliberately.
- [Cargo SemVer compatibility](https://doc.rust-lang.org/cargo/reference/semver.html) — what
  counts as compatible public API evolution.
- [The Rust Reference: unsafe](https://doc.rust-lang.org/reference/unsafe-keyword.html) and the
  [Rustonomicon](https://doc.rust-lang.org/nomicon/working-with-unsafe.html) — unsafe obligations
  and safe-abstraction boundaries.
- [The Rust Book: recoverable errors](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html)
  and [fearless concurrency](https://doc.rust-lang.org/book/ch16-00-concurrency.html) — the
  language's error and ownership model.
