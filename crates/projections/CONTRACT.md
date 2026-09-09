# Optional projection contract

`newton-projections` reflects Newton-owned entity status onto optional external
surfaces. `ProjectionPort` has only a write method. It cannot fetch a board,
enumerate work, approve a Plan, accept a Candidate, or mutate optimizer state.
No-tracker mode calls `NoTracker` directly and requires no credentials, transport,
or journal.

The embedding application supplies canonical entity IDs, monotonically increasing
entity revisions, derived statuses, and authorization for the configured writes.
This library does not derive statuses from external data or grant write authority.
The native optimizer and backend entity store are not wired to this library yet.

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
operator must supply an existing item identity. Automatic issue creation, backend
on-entity link fields, native lifecycle dispatch, configuration/help, and retry
scheduling are remaining integration work, not delivered by this crate.

## Validation

`cargo test -p newton-projections` covers no-tracker mode, durable binding,
duplicate/stale/conflicting revisions, tracker outage and restart, an assignment
applied before response loss, ignored external board edits, independent-instance
locking, path-safe identities, and failed local completion persistence.

`cargo check -p newton-projections --features existing-gh` checks the production
transport wiring without contacting GitHub. No test creates real external items.
