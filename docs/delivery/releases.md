# Release Standard

> Scope: repository releases and release artifacts
>
> Last reviewed: 2026-08-08

This standard defines when a Koduck release is eligible, how it is published,
which evidence it must retain, and how a failed release is contained. The Git
tag rules in [`git-tags.md`](git-tags.md) are part of this standard.

## Authority And Change Control

- A release is an Operational change under [`AGENTS.md`](../../AGENTS.md). An
  Accepted Operational Change Record (OCR) MUST authorize the exact release
  operation before a tag, release, or artifact is published.
- A release MUST use the existing accepted architecture, build pipeline,
  artifact contract, signing configuration, and security boundary. Changing
  any of those is implementation work governed separately by `AGENTS.md` and
  MUST NOT be hidden inside a release OCR.
- `main` is the only release line. Release branches, maintenance branches, and
  independently versioned components MUST NOT be introduced without an
  Accepted Full ADR.
- The operator MUST have explicit authorization for every external write,
  including pushing a tag, publishing a release, uploading an artifact, or
  promoting an image.

## Versioning

Koduck uses [Semantic Versioning 2.0.0](https://semver.org/). Given
`MAJOR.MINOR.PATCH`:

- increment `MAJOR` for an incompatible public contract or behavior change;
- increment `MINOR` for backward-compatible functionality;
- increment `PATCH` for a backward-compatible fix;
- use `alpha.N`, `beta.N`, or `rc.N` prerelease identifiers for artifacts that
  are not yet a stable release.

The public contract includes documented APIs, schemas, protocols, command-line
interfaces, configuration formats, persisted data formats, and supported
operator workflows. A released version MUST never be modified in place. The
corresponding Git tag format is defined in
[`git-tags.md`](git-tags.md#release-tag-format).

## Release Eligibility

A release candidate is eligible only when all of the following are true:

- the exact candidate commit is the current remote `main` tip and is identified
  by its full commit SHA;
- the candidate revision has the required automatic-review coverage and all
  required non-interactive checks pass on that exact revision;
- every decision record governing the candidate is Accepted and its applicable
  implementation evidence is current;
- the release OCR is Accepted and binds the version, candidate commit, expected
  tags and artifacts, execution steps, success criteria, stop condition, and
  recovery or discard path;
- release notes and any migration, compatibility, security, or rollback
  guidance are complete;
- the version and tag are new, ordered after the prior release, and comply with
  the tag standard; and
- the operator's worktree is clean and synchronized with the authorized remote
  state without rewriting local or remote history.

Passing checks on a different commit, a mutable branch name, or an earlier push
does not qualify the candidate.

## Required Release Notes

Every published release MUST identify:

- the version, release tag, and full source commit SHA;
- the governing OCR path;
- user-visible additions, fixes, and removals;
- breaking changes and required migration steps, or an explicit statement that
  there are none;
- known limitations and security-relevant changes;
- each published artifact's immutable identity and checksum or digest; and
- rollback, downgrade, quarantine, or discard guidance appropriate to the
  artifact.

Generated notes MAY be used as a draft, but the operator MUST review them for
completeness and sensitive data before publication.

## Publication Procedure

The Accepted OCR MUST specialize this sequence with exact commands and targets:

1. Discover the actual remote, protected branch, prior release, release
   candidate SHA, signing capability, and publication destination.
2. Confirm every release-eligibility condition and record the check results in
   the OCR.
3. Prepare final release notes and build any required artifacts from the exact
   candidate SHA in a clean environment. Record immutable artifact identities
   and checksums; do not promote them yet.
4. Create and verify the annotated release tag according to
   [`git-tags.md`](git-tags.md), then push only that exact tag ref.
5. Create or update a draft release that references the existing tag, attach
   the verified assets, and re-check the notes, tag target, checksums, and
   attestations.
6. Publish the release. When the hosting platform supports immutable releases,
   immutability SHOULD be enabled and all assets MUST be attached before the
   draft is published.
7. Verify the public tag target, release metadata, downloadable artifacts,
   checksums or digests, and any release-triggered automation. Stop on the first
   failed success criterion.
8. Record the actual immutable results and verification evidence in the OCR.
   Perform any separately authorized promotion or deployment only after the
   release itself is verified.

Do not use a hosting UI that silently creates an unverified tag. Create and
verify the repository tag first, then bind the release to it.

## Failure And Recovery

- Before publication, a failed candidate or local-only tag MAY be discarded as
  defined by the OCR after confirming that no remote tag, release, artifact, or
  promotion exists.
- After a tag is pushed, it is immutable: do not move it, force-push it, delete
  it, or reuse its name. Correct the source and publish a new version.
- If published artifacts are invalid or unsafe, stop promotion, mark the
  release as withdrawn or unsafe without erasing its history, quarantine the
  artifacts, and follow the OCR recovery path. A security incident follows the
  repository's security-response process.
- Deleting a published release or tag is an exceptional destructive operation
  requiring explicit repository-owner authorization and its own Accepted OCR.
  Deletion never permits reuse of the version or tag name.

## Completion Evidence

A release is complete only when the OCR records the source commit, tag object
ID and peeled commit, tag-signature result or approved unsigned-tag exception,
release URL, artifact identities and checksums, verification results, and
promotion or non-promotion disposition. Mutable screenshots and branch names
are supporting evidence only.

## Authoritative References

- [Semantic Versioning 2.0.0](https://semver.org/)
- [Git `tag` documentation](https://git-scm.com/docs/git-tag)
- [GitHub: About releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)
- [GitHub: Immutable releases](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases)
