---
name: liquid-release
description: Publish a Liquid project release for a user-provided semantic version such as 0.2.0. Use when the user asks to release, tag, publish, or bump Liquid to a version; this updates Rust workspace and frontend package versions, validates the build, commits, creates an annotated v-prefixed tag, pushes to origin, and creates a release-v-prefixed GitHub Release.
---

# Liquid Release

## Overview

Release this repository from a version string supplied by the user. Treat `release-v<version>` as the GitHub Release title/name unless the user explicitly asks for a Git branch.

## Inputs

Require a plain `x.y.z` version, for example `0.2.0`.

Use:
- `VERSION=<user version>`
- `TAG=v$VERSION`
- `RELEASE=release-v$VERSION`
- `REMOTE=origin` unless the user specifies another remote

## Preflight

1. Read project instructions first, including `AGENTS.md` and referenced files such as `/Users/zhubby/.codex/RTK.md`.
2. Prefix shell commands with `rtk` in this repository.
3. Confirm the working tree is clean before release changes:

```bash
rtk git status --short --branch
```

4. Confirm the tag and release do not already exist:

```bash
rtk proxy git tag --list "$TAG"
rtk proxy git ls-remote --tags "$REMOTE" "$TAG"
rtk gh release view "$TAG" --repo zhubby/liquid --json tagName,name,url
```

If any existing local tag, remote tag, or release is found, stop and report it.

5. Confirm `gh` is available and authenticated:

```bash
rtk which gh
rtk gh auth status
```

## Version Changes

Update only the release version files:

- `Cargo.toml`: `[workspace.package] version`
- `Cargo.lock`: regenerated/synchronized by Cargo
- `liquid-ui/package.json`: top-level `version`

All workspace crates inherit `version.workspace = true`, so do not edit individual crate manifests unless the project changes.

Use `apply_patch` for manifest edits. Then run Cargo once to refresh `Cargo.lock`:

```bash
rtk cargo check --workspace
```

If frontend build changes `liquid-ui/next-env.d.ts`, inspect it. Revert that generated local noise unless the user explicitly asked to keep it.

## Validation

Run these checks before committing:

```bash
rtk cargo check --workspace
rtk cargo test --workspace
rtk bun run lint
rtk bun run build
```

Run the Bun commands from `liquid-ui/`.

After validation, confirm the diff contains only expected release changes:

```bash
rtk git status --short --branch
rtk proxy git diff --stat
rtk proxy git diff -- Cargo.toml Cargo.lock liquid-ui/package.json
rtk proxy git diff --check
```

## Commit, Tag, Push, Release

Create one release commit:

```bash
rtk git add Cargo.toml Cargo.lock liquid-ui/package.json
rtk git commit -m "chore(release): bump version to $VERSION"
```

Fetch before tagging to ensure `origin/main` did not move:

```bash
rtk git fetch origin main --tags
rtk git status --short --branch
```

If the branch is behind or diverged, stop and report it. If it is clean and ahead only by the release commit, create the annotated tag:

```bash
rtk git tag -a "$TAG" -m "$RELEASE"
```

Push main and the tag:

```bash
rtk git push origin main
rtk git push origin "$TAG"
```

Create the GitHub Release:

```bash
rtk gh release create "$TAG" --repo zhubby/liquid --title "$RELEASE" --generate-notes
```

## Final Verification

Verify local and GitHub state:

```bash
rtk git status --short --branch
rtk proxy git log -1 --oneline --decorate
rtk proxy git cat-file -p "$TAG"
rtk gh release view "$TAG" --repo zhubby/liquid --json tagName,name,isDraft,isPrerelease,publishedAt,url,targetCommitish
```

Final response must include:

- Files updated.
- Commit hash and message.
- Tag name.
- GitHub Release URL.
- Validation commands and results.
- Whether the working tree is clean.

Emit git directives only for actions that actually succeeded.
