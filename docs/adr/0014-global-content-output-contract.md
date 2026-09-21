# Global Content Output Contract Recovery

Status: accepted

The global content-processing contract uses declared target fields at the top level as the canonical content output. Before validation, the runtime may normalize only an unambiguous single Markdown code fence or single `output` envelope; it must reject unknown or ambiguous wrappers and persist only declared fields. Truncated provider responses are a separate failure class with bounded recovery. The incident itself does not get a special requeue path: work uses the existing automatic recovery policy while its retry window is open, and an authorized user retry may reopen an expired window.

**Considered Options**

- Require providers to immediately emit only the canonical shape: rejected because the current provider already emits deterministic wrappers and the existing incident would remain stuck until every upstream behavior changes.
- Accept arbitrary JSON shapes: rejected because it can silently publish the wrong fields or control metadata.
- Add an incident-specific batch requeue operation: rejected because this incident does not justify a new recovery authority or a production-data repair path.

**Consequences**

- Provider success and business output success remain separate facts.
- Existing wrapped responses can be recovered without changing the persisted result contract.
- Truncated outputs cannot consume the same unbounded retry path as ordinary contract mismatches.
- The confirmed cohort is observation scope only; recovery remains owned by the normal scheduler and existing authorized retry path.
