# Dependency planning

Newton can answer: “Which Modules must be re-released to carry this change to my
Product?” It reads a human-approved dependency map and returns a deterministic
Impact Sequence. It does not assign versions, execute releases, or prove that
changes reached production. No server, agent, or portfolio database is required.

All commands below emit JSON. Errors go to stderr and exit nonzero. Inspection,
discovery, and impact queries do not change their input files. `approve` creates a
new Baseline file and refuses to overwrite an existing one.

## First map

The [example files](../examples/dependency-planning/) contain a complete small
portfolio: `base → middle → app`, with `app` owned by `product`, and a separate
unrelated consumer of `base`. Run these commands from a checkout containing the
examples, or download the files and adjust their paths.

```bash
newton dependency inspect --map examples/dependency-planning/map.json
newton dependency discover \
  --map examples/dependency-planning/map.json \
  --manifest examples/dependency-planning/Cargo.toml \
  --catalog examples/dependency-planning/catalog.json \
  --owner app > reviewed-map.json
```

`map.json` contains `artifacts`, `memberships`, `dependencies`, and `issues`.
Artifact IDs are explicit catalog identities. Membership follows Product →
Component → Repo → Module. An edge's `from` consumes its `to`; the release order
therefore runs in the opposite direction. The Cargo catalog maps package names
and registry/path/Git sources to those exact Module identities and versions.
Mismatched map/catalog versions fail instead of being silently updated.

Discovery refreshes only Detected edges from the supplied manifest. Human
Declared relationships and unreviewed Suggested relationships survive. Run
discovery once per package manifest, passing the preceding review document as
`--map` and saving each result to a different file. An approved Baseline can also
be used as map input, but rediscovery discards its approval.

The output includes the canonical `map`, `map_fingerprint`, `approval_required:
true`, and source-local `discovery_report`. Missing catalog packages, ambiguous
identities, and unsupported inputs become explicit issues. Rerunning the same
manifest against the same map is idempotent.

## Human review and durable approval

A human must review the whole map, add missing cross-service/API/schema
relationships as Declared edges, and decide whether unresolved inputs are
acceptable for the intended scope. Detected/Declared edges are individually
Confirmed; that does not establish map completeness. Suggested edges do not
influence automatic sequencing until an authorized human promotes them to
Declared.

Inspect the final map again after edits:

```bash
newton dependency inspect --map reviewed-map.json
```

The authorized reviewer supplies `human-review.json`:

```json
{
  "map_fingerprint": "COPY_THE_EXACT_REVIEWED_FINGERPRINT",
  "reviewed_by": "your-authorized-reviewer-identity",
  "reviewed_at": "2026-09-09T00:00:00Z",
  "completeness_statement": "Reviewed package and cross-service dependencies for this product.",
  "acknowledged_issues": []
}
```

Use the actual review time and scope. If issues remain, `acknowledged_issues` must
contain every exact issue ID from the reviewed map. Acknowledgement records a
known limitation; it does not repair the missing relationship. Those limitations
remain visible in subsequent Impact Sequences.

```bash
newton dependency approve --map reviewed-map.json \
  --review human-review.json --output baseline.json
```

This validates and packages an existing human-issued review. It does not
authenticate the reviewer or authorize an agent to impersonate one. The local
operator owns approval authorization and must protect review/Baseline files from
unauthorized edits. A fingerprint is content identity, not a signature.
`approve` is deliberately excluded from MCP and chat tools.

The Baseline stores canonical facts and their exact approval. Writes use a
synced temporary file and no-clobber publication; Unix also syncs the containing
directory. Preserve the document in durable, access-controlled storage. A new
review produces a new file; existing Baselines are immutable through this CLI.
Malformed, altered, or unapproved documents fail before planning.

## Ask for an Impact Sequence

```bash
newton dependency impact --baseline baseline.json --changed base --target product
newton dependency impact --baseline baseline.json --changed base --target product \
  --changes examples/dependency-planning/changes.json
```

The example produces dependency-first stages for `base`, `middle`, and `app`;
`unrelated` is excluded. Compatible hops remain in the sequence because each hop
must carry the change forward. Cycles become explicit Co-release Groups, never an
arbitrary linear order.

`--changes` accepts a JSON map of project-owned proposed versions and/or explicit
compatibility signals keyed by Module ID. It does not persist or assign versions.
Without justified per-hop facts, effort remains `unknown`, which the planner must
treat as requiring adaptation. Semver can supply compatibility information;
commits, CalVer, and opaque labels do not imply compatibility.

Output includes `baseline_fingerprint`, `reaches_target`, ordered `stages`,
`unknown_compatibility`, and `discovery_limitations`. No confirmed path produces
`reaches_target: false` and no stages, not a claim that discovery was complete.
The same inputs produce byte-stable JSON.

## Planner tools and support limits

The existing Newton MCP registration exposes `newton_dependency_inspect` and
`newton_dependency_impact`. They use the same command handlers and validation as
the CLI. For example, `newton_dependency_impact` accepts:

```json
{"baseline":"/workspace/baseline.json","changed":"base","target":"product"}
```

Existing MCP transport authentication/exposure rules still apply. Discovery and
approval are local commands, not additional remotely exposed tools. These
commands do not add an execution runtime or HTTP work-ingress endpoint.

Supported discovery is direct Cargo.toml package dependencies, including explicit
registry/path/Git sources, aliases, and the conservative union of optional and
target-specific edges. Workspace inheritance, lockfile resolution, source
overrides, Cargo config replacement, and other ecosystems are not silently
inferred. Unsupported `--ecosystem` values fail. See the complete
[adapter support matrix](../crates/dependencies/CONTRACT.md#cargo-discovery-support-matrix).

These local Baseline documents are not the portfolio SQL catalog adapter.
`newton data` Module/ModuleDependency rows are not automatically loaded, upgraded,
or synchronized by this command. The caller supplies explicit map facts and
versions. No SQL migration or parallel release-execution store is implied.
