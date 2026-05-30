// credit_approved requires a case_id in metadata
@id("credit-requires-case-id")
forbid (
  principal is Recall::Agent,
  action == Recall::Action::"WriteMemory",
  resource is Recall::MemoryEntry
) when {
  resource.event == "credit_approved" &&
  !context.metadata.hasAttribute("case_id")
};

// No agent may write PII-tagged entries to external scope
@id("no-pii-external")
forbid (
  principal,
  action == Recall::Action::"WriteMemory",
  resource
) when {
  resource.tags.contains("pii") &&
  resource.scope == "external"
};

// Fraud flag on high-value account requires supervisor countersign
@id("fraud-flag-high-value-supervisor")
forbid (
  principal,
  action == Recall::Action::"WriteMemory",
  resource
) when {
  resource.event == "flag_suspicious" &&
  context.entity_account_value_usd_cents > 1000000
} unless {
  context.has_supervisor_countersign
};

// Only trust-level 3+ agents may write credit_approved
@id("credit-approved-trust-level")
forbid (
  principal is Recall::Agent,
  action == Recall::Action::"WriteMemory",
  resource is Recall::MemoryEntry
) when {
  resource.event == "credit_approved" &&
  principal.trust_level < 3
};

// Quarantined agents cannot write
@id("quarantined-no-write")
forbid (
  principal is Recall::Agent,
  action == Recall::Action::"WriteMemory",
  resource
) when {
  principal.enforcement_stage == "QUARANTINE" ||
  principal.enforcement_stage == "EVICT"
};

// Default permit — explicit, never implicit
@id("default-permit")
permit (principal, action, resource);
