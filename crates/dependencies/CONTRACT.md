# Dependency planning contract

`newton-dependencies` computes release-planning facts over an explicitly approved
dependency map. It has no database, HTTP, subprocess, filesystem, or LLM boundary.
Embedding applications own catalog loading, durable persistence, human
authentication/authorization, and planner-tool exposure. This crate does not
execute releases or prove that a change reached production.

## Public planner path

1. Construct a `DependencyGraph` from a `DependencyMap`. Invalid identities,
   hierarchy links, Discovery metadata, and version constraints fail immediately.
2. Parse supplied manifest text using `discover_cargo`. Resolve package identities
   through an explicit catalog; review unresolved `DiscoveryIssue` records.
3. Apply `refresh_detected`. Only the report's source is refreshed. Declared and
   Suggested edges and facts from other sources survive. Repeating a report is
   idempotent. Refresh produces an unapproved graph, never an inherited approval.
4. Show the canonical map and `fingerprint()` to an authorized human. Record
   `BaselineApproval`, including a completeness statement and acknowledgement of
   every unresolved issue ID. Individual Detected edges are confirmed facts, not
   proof that the whole map is complete.
5. Call `ApprovedBaseline::approve`. The fingerprint must identify exactly the map
   reviewed. Persist `document()` through the application's storage boundary.
   `from_document` revalidates graph facts, fingerprint, and approval metadata.
6. Call `impact_sequence(&ImpactRequest)`. The public API integration tests show
   this path without substituting a test implementation of the algorithms.

A fingerprint detects changed facts; it is not a signature or an authorization
system. The embedding application MUST authenticate the reviewer and control
baseline writes. Approval acknowledges unresolved limitations; it does not make
them disappear. Results include those limitations for the planner.

## Topology and effort

Artifact IDs are globally unique catalog identities, not package names. Ownership
is Product → Component → Repo → Module, with one direct parent per child. A target
may be any of those levels. This implementation accepts Module-to-Module
dependencies; Repo-to-Repo dependency edges are explicitly rejected, not ignored.

An edge points from a dependent to the Module it consumes. Only Detected and
Declared edges participate. Suggested edges remain visible in the reviewed map
but require authorized promotion to Declared before they can affect sequencing.

Propagation intersects dependents reachable from the changed Module with
dependencies reachable from the target's owned Modules. This includes compatible
hops and excludes unrelated products. A change with no confirmed path produces
`reaches_target: false` and no release stages. An empty result does not prove that
the discovery map is complete.

Strongly connected components are indivisible Co-release Groups, including
self-dependencies. Groups are ordered dependency-first into parallel stages.
Member order within a group is presentation order, not permission to linearize a
cycle. Output and map fingerprints are stable across input ordering and identical
duplicate edges. Algorithms use iterative graph traversal and support portfolios
beyond 100 nodes; construction/canonicalization adds sorting to linear graph work.

The request supplies optional project-owned planned versions or explicit
compatibility statements for each Module. At each hop, effort follows the changes
to its incoming dependencies: Breaking → Adapt, NonBreaking → BumpOnly, and
Unknown → Unknown with `requires_adaptation() == true`. Breaking dominates
Unknown; Unknown dominates NonBreaking. For a starting Module without incoming
propagation edges, effort reflects its own change signal.

Do not infer downstream release compatibility from the initial change alone.
Absent downstream version/signal facts remain visibly Unknown. Semver increases
derive compatibility, with pre-1.0 incompatible increments handled conservatively.
Pre-releases, unchanged versions, downgrades, CalVer, commits, and opaque versions
do not supply an implicit non-breaking signal. An explicit project-owner signal
takes precedence. Newton never assigns the next version.

Range constraints require Semver targets. Exact constraints require the same
scheme; Pinned constraints require Commit targets. CalVer ordering/ranges are not
implemented. Constraint expressiveness is validated, but constraint satisfaction
never removes a propagation hop.

## Cargo discovery support matrix

| Input | Behavior |
| --- | --- |
| One package's Cargo.toml text | Supported; package name and supplied Semver version must match its catalog owner. |
| Runtime/build/dev dependency tables | Supported; mapped to separate dependency kinds. |
| Registry version strings and tables | Supported through explicit package plus registry catalog identity. |
| Renamed dependencies (`package`) | Resolved using the actual package name. |
| Path dependencies | Lexically resolved against the absolute manifest path; catalog paths must also be absolute. |
| Git URLs and branch/tag/rev selectors | Matched exactly against catalog source identity; no URL/name guessing. |
| Full Git SHA without package-version constraint | Pinned when the catalog uses Commit versions. |
| Optional and target-specific dependencies | Conservative union of all declared edges; feature/cfg pruning is not implemented. |
| Missing or ambiguous catalog identity | Unresolved issue, no inferred edge. |
| Workspace-inherited dependency/version | Explicit unresolved issue; no workspace resolution is implied. |
| Virtual workspace | Explicit issue directing the caller to scan each member's manifest. |
| `[patch]` / `[replace]` | Explicit unsupported-source-override issue; no edges are guessed. |
| Combined package-version and exact Git pin | Unresolved when the single-version graph cannot represent both constraints. |
| Cargo.lock, Cargo config source replacement, symlink resolution | Not implemented; caller must resolve these inputs or record limitations during review. |
| npm, Go, Maven, service/API/schema dependencies | Not discovered; provide Declared edges or a separately supported adapter. |

The adapter parses untrusted text without executing it. It does not run build
scripts, Cargo, Git, or network requests. Callers supply catalog facts and manifest
contents from the reviewed revision. An empty issue list only describes this
adapter's supported input surface; it never establishes complete cross-service
discovery or an approved Baseline.

## Validation

`cargo test -p newton-dependencies` exercises the public boundary, a real manifest
fixture, target scoping, confirmation/promotion, rediscovery, approval tampering,
version strictness, parallel stages, SCCs, and a 150-Module portfolio.
