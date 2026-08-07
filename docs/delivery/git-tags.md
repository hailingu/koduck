# Git Tag Standard

> Scope: local and remote Git tags in the Koduck repository
>
> Last reviewed: 2026-08-08

This standard makes release tags unique, auditable, and immutable. Read it
together with [`releases.md`](releases.md) and the operational gates in
[`AGENTS.md`](../../AGENTS.md).

## Release Tag Format

Release tags MUST use this repository-wide format:

```text
vMAJOR.MINOR.PATCH
vMAJOR.MINOR.PATCH-(alpha|beta|rc).N
```

The exact validation expression is:

```regex
^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-(alpha|beta|rc)\.(0|[1-9][0-9]*))?$
```

Valid examples are `v0.1.0`, `v1.4.2`, and `v2.0.0-rc.1`. Invalid examples
include `1.4.2` (missing `v`), `v1.4` (incomplete), `v01.4.2` (leading zero),
`v1.4.2+build.7` (build metadata), and `api/v1.4.2` (component scope).

- Stable releases MUST use the three-part form without a suffix.
- Prereleases MUST use `alpha.N`, `beta.N`, or `rc.N`, with `N` starting at 1
  for each version and increasing without reuse.
- Build metadata (`+...`), date tags, floating tags such as `latest`, partial
  tags such as `v1`, and component-scoped tags are prohibited.
- The repository has one version sequence. Independent component versioning
  requires an Accepted Full ADR before the first component tag is created.

## Tag Object Requirements

- A release tag MUST be an annotated tag object that ultimately points to a
  commit. Lightweight tags MUST NOT represent releases.
- A release tag SHOULD be cryptographically signed with a verifiable identity.
  If approved signing capability is unavailable, the decision owner MUST record
  a written exception and its reason in the release OCR before an unsigned
  annotated tag is created.
- The tag annotation MUST include the release version, the full target commit
  SHA, and the repository-relative path of the governing OCR.
- The tagger MUST name the exact commit explicitly. Creating a release tag from
  an implicit `HEAD`, branch name, or hosting UI default is prohibited.
- The tag's author, timestamp, message, signature result, object ID, and peeled
  commit ID are release evidence and MUST be retained in the OCR.

## Preflight

Before creating a release tag, the operator MUST confirm and record:

- the Accepted release OCR and its exact approved revision;
- a clean worktree and the intended remote;
- the full candidate SHA and that it equals the current remote `main` tip;
- successful checks and automatic-review coverage for that exact SHA;
- the prior release tag and the resulting Semantic Versioning increment;
- absence of the proposed tag name both locally and on the remote; and
- availability and identity of the approved signing method, or the approved
  unsigned-tag exception.

Representative read-only checks are:

```sh
git status --short --branch
git remote -v
git rev-parse --verify '<release-sha>^{commit}'
git rev-parse --verify refs/remotes/origin/main
git tag --list '<tag>'
git ls-remote --tags origin 'refs/tags/<tag>' 'refs/tags/<tag>^{}'
```

Remote discovery or synchronization MUST be explicitly authorized. An empty
local and remote tag lookup is required before creation.

## Creation And Publication

Use a signed annotated tag when approved signing is available:

```sh
git tag -s '<tag>' '<release-sha>' \
  -m 'Koduck <tag>' \
  -m 'Commit: <release-sha>' \
  -m 'OCR: docs/adr/ocr/OCR-NNNN-<slug>.md'
git tag --verify '<tag>'
git push origin 'refs/tags/<tag>:refs/tags/<tag>'
```

When the OCR contains an approved unsigned-tag exception, replace `-s` with
`-a` and record that signature verification was not applicable. Placeholders
MUST be replaced with the exact approved values. Push only the intended tag;
`git push --tags` is prohibited.

After the push, verify that the remote annotated tag peels to the approved
commit and record both returned object IDs:

```sh
git ls-remote --tags origin 'refs/tags/<tag>' 'refs/tags/<tag>^{}'
git show --no-patch --format=fuller '<tag>'
```

The release MUST NOT proceed if the tag is lightweight, unsigned without the
recorded exception, points to the wrong commit, differs locally and remotely,
or fails signature verification.

## Immutability And Corrections

- A pushed tag name and target are permanent. Never force, move, delete,
  recreate, or reuse a published tag.
- Commands that can overwrite published tags, including `git tag -f` and a
  forced tag push, are prohibited.
- A local-only tag MAY be deleted and recreated inside the same authorized OCR
  only after confirming it was never pushed or otherwise published.
- A mistake discovered after publication MUST be corrected in a new patch or
  prerelease version. Release notes for the affected version MUST warn users
  without rewriting the historical tag.
- Exceptional deletion follows the destructive-operation rule in
  [`releases.md`](releases.md#failure-and-recovery) and never makes the name
  reusable.

## Non-Release Tags

Temporary local tags MAY be used for private investigation when they cannot be
confused with the release-tag format. They MUST NOT be pushed and SHOULD be
deleted when the investigation ends. Any remote automation, environment,
promotion, or floating tag requires an Accepted Full ADR because it defines a
new repository or delivery contract.

## Authoritative References

- [Semantic Versioning 2.0.0](https://semver.org/)
- [Git `tag` documentation](https://git-scm.com/docs/git-tag)
- [GitHub: Immutable releases](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases)
