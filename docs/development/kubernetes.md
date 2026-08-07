# Kubernetes Development Standard

**Applies to**: any Kubernetes manifest, overlay, or operational change in this
repository.

**Last reviewed**: 2026-08-07

## Required Reading

- [Kubernetes Configuration Good Practices](https://kubernetes.io/docs/concepts/configuration/overview/) —
  official guidance on manifest structure, version control, labels, and
  workload/service configuration.
- [Kubernetes Security](https://kubernetes.io/docs/concepts/security/) —
  official overview of control-plane protection, secrets, workload
  protection, admission control, and auditing.
- [Considerations for Large Clusters](https://kubernetes.io/docs/setup/best-practices/cluster-large/) —
  official guidance for scaling considerations once a cluster grows; consult
  when sizing or scaling decisions are in scope.

## Baseline Practices

- Store manifests in version control as YAML; never apply an unreviewed
  manifest directly to a live cluster.
- Never hard-code secrets, tokens, or credentials in a manifest; use the
  project's configured secret mechanism.
- A manifest or deployment topology change is normative configuration under
  this repository's Decision Records policy — classify it before editing, not
  after.

## Before Writing Code

Read this file, then inspect the target overlay/namespace for existing
labels, resource limits, and security context conventions, and match them.
