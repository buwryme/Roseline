# release notes

Each release needs a notes file here, named after the version **without** the
leading `v`:

```text
docs/releases/0.1.1-hotfix1.md
```

The release workflow refuses to run if the file is missing or empty, and uses
it verbatim as the GitHub release body. The file must be part of the commit
being released.

The version input must match `Cargo.toml` and `Cargo.lock`, and must be
`X.Y.Z` or `X.Y.Z-hotfixN`. The workflow creates the matching `v` tag on the
specified commit.

Package recipes are versioned from that input during the build; the source
commit itself only needs its cargo version bumped.

## release token

GitHub's built-in `GITHUB_TOKEN` cannot create a tag or release when the target
commit touches `.github/workflows`. Add a `RELEASE_TOKEN` repository secret with
a fine-grained PAT that has **Contents: write** and **Workflows: write**; the
workflow uses it automatically. Without it, build jobs still run but publishing
the release fails at the tag step.
