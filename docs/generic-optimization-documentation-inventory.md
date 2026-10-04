# Generic optimization documentation inventory

| Surface | Authority | Disposition and check |
| --- | --- | --- |
| README and quick start | `README.md` | Updated to the generic loop and JSON result path; links checked locally. |
| CLI help and arguments | Rust constants/types | Updated role names, strategy behavior, persistence, and poll wording; CLI help tests exercise rendering. |
| Optimization contract | Rust wire types and driver | Rewritten with definition v2, all workflow outputs, acceptance, stopping, history, and recovery. |
| Architecture and glossary | `architecture.md`, `CONTEXT.md` | Updated from accepted target language to implemented domain-neutral behavior. |
| ADRs | `docs/adr/0016-*`, `0017-*` | Retained and marked implemented; historical alternatives preserved. |
| Migration/release guidance | `docs/generic-optimization-migration.md` | Added explicit breaking changes and old-run policy. |
| Review closure | `docs/generic-optimization-review-resolution.md`, `docs/generic-optimization-implementation-review.md` | Separates the historical spec review from the later implementation review; maps defects to fixes and tests. |
| Testing and real agent | `docs/testing-generic-optimization.md` | Added deterministic, same-binary, Pi, Docker, gateway-evidence, and diagnostic guidance. |
| Delivery validation | `docs/generic-optimization-validation.md` | Preserves initial delivery results and records the full-suite, Docker and isolated-skill follow-up, with live inference still explicitly unexercised. |
| Realtime and serve | `docs/realtime.md`, Newton serve skill reference | Clarified events are invalidation hints and generic local history is file-backed; SQLite API is optional/separate. |
| Generated schemas | `newton_core::optimization::schema` | Added schemas for workflow outputs, Cycle history, and report; compiled in tests. |
| Canonical Newton skill | `skill/newton/` | Updated generic authoring guidance and references. |
| Distributed skill | `.agents/skills/newton/` | Synchronized and checked byte-for-byte. Offline install and locked restore of the canonical skill pass in a disposable project; unrelated repository-wide legacy lock entries remain unchanged. |
| Built-in template | `resources/newton-template/newton/` | Updated to definition v2 and propose/execute outputs; validated by test/build paths. |
| Pre-version-2 CLI fixtures/tests | old `tests/fixtures/optimization` and included modules | Retired; equivalent generic public-CLI coverage lives in the scheduling fixture and `test_e2e_optimize`. |
| Historical draft/review | `docs/draft/` | Retained as historical design evidence; current documents identify implemented behavior. |
| Legacy catalog operator docs | `docs/operators/` | Retained for optional Findings/Plans APIs; no longer presented as optimizer prerequisites. |

External installed copies can be refreshed with the repository's FastSkill
project configuration after release. This change updates only repository-owned
canonical and distributed copies.
