# chrono-ta public release

## Decision

Rename the public project from `ta-rs-improved` to `chrono-ta` and publish it as
the `chrono-ta` package. The default Rust crate name becomes `chrono_ta`.

The old name described the project only in relation to its upstream. The new
name describes the actual contract: technical indicators whose windows and
updates are driven by timestamps and elapsed time.

## Compatibility

Existing Git dependencies pinned to an old commit remain valid after the GitHub
repository rename because GitHub redirects clone, fetch, and push traffic. Those
commits still contain a package and library named `ta`.

New consumers use `chrono_ta`:

```toml
[dependencies]
chrono-ta = { git = "https://github.com/austin-starks/chrono-ta" }
```

A consumer that wants to preserve existing `use ta::...` imports can rename the
dependency locally:

```toml
[dependencies]
ta = { package = "chrono-ta", git = "https://github.com/austin-starks/chrono-ta" }
```

This project is not a drop-in replacement for upstream `ta`. Its constructors
accept `Duration`, and `Next` consumes `(DateTime<Utc>, value)` rather than only
a value or `DataItem`.

## Public-release work

- Replace the README with an accurate comparison, installation paths, runnable
  examples, supported indicators, time-bucket semantics, and limitations.
- Give Cargo a unique package name and accurate metadata.
- Include the license, changelog, and examples in the packaged crate.
- Add CI, contribution guidance, security reporting, issue templates, and
  coding-agent guidance.
- Remove the stale Travis and missing-benchmark declarations.
- Verify formatting, all-feature tests, examples, documentation, and the exact
  package contents before any registry upload.

## Release sequence

1. Land the rename and public-release files on the default branch.
2. Rename the GitHub repository to `chrono-ta` and update local remotes.
3. Re-run release verification from a clean checkout using the new URL.
4. Create or verify a crates.io account and API token for the maintainer.
5. Run `cargo publish --dry-run`, inspect the archive, then publish deliberately.
6. Confirm the crate page and docs.rs build before announcing the registry path.

Do not reuse `austin-starks/ta-rs-improved` after the GitHub rename; doing so
would remove GitHub's redirect for existing consumers.
