# Dependency planning

Use `newton dependency impact --baseline /path/baseline.json --changed MODULE_ID
--target TARGET_ID` to get a deterministic JSON Impact Sequence from a human-approved
local Baseline. The corresponding MCP tool is `newton_dependency_impact`, with
`baseline`, `changed`, `target`, and optional `changes` arguments.

- Agents MAY inspect maps and compute Impact Sequences.
- Only Detected/Declared edges are Confirmed. Suggested edges MUST NOT determine
  release order before authorized human promotion.
- Agents MUST NOT invent reviewer identities, issue acknowledgements, or human
  approval records. `dependency approve` packages an already authorized human
  review and is deliberately unavailable through MCP/chat.
- Unknown compatibility requires adaptation allowance. Agents MUST NOT replace
  it with guessed versions or treat an empty sequence as completeness proof.
- Co-release Groups require explicit planning resolution; presentation order is
  not execution order inside a cycle.

`dependency discover` reads one real Cargo.toml against an explicit map and package
catalog. It refreshes that source's Detected facts, preserves human declarations,
reports unresolved inputs, and emits an unapproved review document. `dependency
inspect` validates a map and reports its canonical fingerprint. Only inspect and
impact are exported as MCP tools.

The Baseline is a durable local document, not synchronized portfolio SQL rows.
For map/review formats, runnable examples, and adapter limits, see
[the dependency planning guide](../../../docs/dependency-planning.md) and each
command's `--help`.
