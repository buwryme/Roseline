# release notes

Each release needs a notes file here, named after the version **without** the
leading `v`:

```text
docs/releases/0.1.0.md
```

The release workflow refuses to run if the file is missing or empty, and uses
it verbatim as the GitHub release body. The file must be part of the commit
being released.

The version input must match `Cargo.toml` and `Cargo.lock`, and must be plain
`X.Y.Z`. The workflow creates the `vX.Y.Z` tag on the specified commit.

Package recipes are versioned from that input during the build; the source
commit itself only needs its cargo version bumped.
