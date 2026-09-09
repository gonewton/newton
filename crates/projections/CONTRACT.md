# Optional projection contract

`newton-projections` reflects Newton-owned entity status onto optional external
surfaces. `ProjectionPort` has only a write method. It cannot fetch a board,
enumerate work, approve a Plan, accept a Candidate, or mutate optimizer state.
No-tracker mode calls `NoTracker` directly and requires no credentials, transport,
or journal.

The embedding application supplies canonical entity IDs, monotonically increasing
entity revisions, derived statuses, and authorization for the configured writes.
This library does not derive statuses from external data or grant write authority.
`RunProjectionService` supplies bounded delivery for immutable native Run
snapshots. The CLI integration freezes optional configuration in the Run journal
before work and reads only the durably recorded internal terminal status when
projecting it. It does not patch the backend Run, Cycle, Plan, or Change Request.

## Durable identity and delivery

`ProjectionDispatcher` uses the storage-independent `ProjectionStore` and
`ProjectionLease` contracts. A delivery lease MUST serialize writers across
processes for the same entity/destination. Successful saves MUST be atomic and
durable. `FileProjectionStore` supplies local-file locks, hashed identity paths,
temporary-file replacement, file fsync, and directory fsync on Unix. Its directory
must be on a local filesystem with working advisory locks and atomic replacement.
It is a projection journal, not a second store of optimization work.

1. The caller explicitly binds an existing external identity to an internal
   entity. The binding is persisted before any remote write. Repeating it is
   idempotent; changing it silently is rejected.
2. Delivery persists the pending revision/status before calling the adapter.
3. Successful assignment persists the delivered revision and clears pending.
4. An adapter outage records a projection-only diagnostic and returns `Deferred`.
   The caller may retry later; internal work and acceptance do not depend on it.
5. A crash or failed completion save leaves a durable pending assignment. Retry
   repeats only idempotent state assignment on the same existing resource.

An already delivered revision performs no remote call. Older revisions cannot
replace a newer local pending/delivered revision. The same revision with a
different payload is an error. A newer revision can supersede failed pending
delivery. A completion persistence failure returns an error, never a successful
delivery claim.

This is not an exactly-once external execution guarantee. GitHub may apply an
assignment before a response is lost, and humans may subsequently edit the board.
Repeating the same assignment is safe because no resource is created. Local
serialization cannot guarantee remote ordering after arbitrarily delayed requests
or process crashes; later authoritative assignments restore the desired view.
Delivery metadata records responses, not a permanent claim about remote state.
None of these external conditions can change the optimizer's authoritative work.

## GitHub adapter

`GithubProjectProjection` uses the existing `gh project item-edit` capability with
explicit Project, item, field, and single-select option IDs. The status-to-option
map is supplied in the authorized binding. Missing identity or status mapping is
an error; the adapter never guesses, creates an issue/item, searches issue bodies,
lists a board, or reads external state.

The optional `existing-gh` feature provides `with_existing_gh(workspace)`, reusing
Newton's current `GhRunner` and its guarded subprocess cleanup. The call has a
30-second timeout and persists only a generic diagnostic, not arbitrary gh stderr.
Tests inject the same command seam with a deterministic fake tracker.

Initial external resource creation is deliberately unsupported: a non-idempotent
GitHub create can succeed before the response is lost, so automatic retry could
create duplicates without a server-supported idempotency mechanism. An authorized
operator must supply an existing item identity. Automatic issue creation and
backend on-entity link fields remain unsupported. The initial native hook
projects terminal Optimize Run status, not every intermediate Plan/CR transition.

## Native Run integration

The optional `.newton/projections.json` file declares version `1`, existing
destinations, `max_delivery_attempts` (default 8), and a shared outbound
`timeout_seconds` budget (default 5). At most 64 destinations and 64 attempts are
accepted; timeout values must be 1–60 seconds. Configuration is declarative JSON.
See [the user contract](../../docs/optimization-contract.md#optional-status-projection)
for a complete example.

The native integration stores the selected configuration, including the absence
of configuration, in the existing Run journal before agent work. Resume uses that
snapshot rather than reloading the source. No-tracker operation needs no
projection journal, network transport, credentials, or publication authority.
Configured GitHub writes require the host's existing `command`, `network`, and
`publish` authority. A publication grant cannot bypass a command/network denial;
a source file does not grant any of these permissions.

After the optimizer persists a terminal outcome, the adapter reads that Run from
Newton's store and reflects its status. Terminal snapshot revision is
`cycle + 1`; repeating the terminal hook reuses that revision. Conflicting final
statuses in the same Cycle fail projection rather than silently overwrite one
another. The hook cannot be used for arbitrary intermediate phase updates without
introducing a corresponding durable event revision.

Native budget exhaustion uses the `resource_limit` status for cycle, work,
evaluation, and elapsed-time limits. Status-option bindings must map that key,
not the historical `max_cycles` label. It remains separate from `converged`;
projecting a stop never asserts completion or changes the saved stopping reason.

Delivery bindings and write-ahead assignments live under the resolved state
root's `projections/delivery` directory. The separate `projection-report.json`
beside the Run journal records delivered, duplicate, deferred, or locally failed
projection results. These results do not alter optimization acceptance or completion.
Only pre-execution configuration diagnostics are frozen in the journal itself.
Repeating terminal synchronization retries a deferred assignment safely.
The finished-run retry shim reopens the same store, reads the frozen journal
configuration, and writes only delivery metadata and `projection-report.json`
beside the journal. It does not acquire, release, or change optimization ownership
or mutate the saved Run journal/outcome. A standalone scheduler/daemon is not
required or supplied.

The shared timeout bounds outbound calls. Targets beyond the attempt/time budget
still receive durable pending assignments, without a remote call. A later retry
continues them; already delivered/idle targets do not consume transport attempts.
Local filesystem durability is required independently and is not advertised as a
hard real-time operation.

## Validation

`cargo test -p newton-projections` covers no-tracker mode, durable binding,
duplicate/stale/conflicting revisions, tracker outage and restart, an assignment
applied before response loss, ignored external board edits, independent-instance
locking, path-safe identities, and failed local completion persistence.

`cargo check -p newton-projections --features existing-gh` checks the production
transport wiring without contacting GitHub. No test creates real external items.

The service tests additionally cover finite timeout/attempt budgets and pending
delivery progress. CLI integration tests use the real SQLite backend and native
Run lifecycle, with only the existing `gh` boundary replaced by a fake. They
verify absent and frozen configuration across resume, outage recovery, duplicate
delivery, ignored board edits, and unchanged authoritative Run/Cycle records.
