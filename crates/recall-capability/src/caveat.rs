use chrono::{Timelike, Utc};
use recall_proto::capability as cap_proto;

/// Evaluate a single caveat. Returns Ok(()) if satisfied, Err with reason if not.
pub fn evaluate_caveat(caveat: &cap_proto::Caveat, context: &CaveatContext) -> Result<(), String> {
    let spec = match &caveat.spec {
        Some(s) => s,
        None => return Err("caveat missing spec".into()),
    };

    match spec {
        cap_proto::caveat::Spec::TimeOfDay(c) => {
            let hour = Utc::now().hour() as i32;
            if hour < c.utc_hours_start || hour >= c.utc_hours_end {
                return Err(format!(
                    "TimeOfDayCaveat: current hour {} not in [{}, {})",
                    hour, c.utc_hours_start, c.utc_hours_end
                ));
            }
        }
        cap_proto::caveat::Spec::RateLimit(c) => {
            if context.actions_in_window >= c.max_actions {
                return Err(format!(
                    "RateLimitCaveat: {} actions used, max {} per {}s window",
                    context.actions_in_window, c.max_actions, c.window_seconds
                ));
            }
        }
        cap_proto::caveat::Spec::TagCaveat(c) => {
            match caveat.r#type {
                // NeverIfTaggedCaveat
                3 => {
                    if context.entry_tags.contains(&c.tag.as_str())
                        && (c.scope.is_empty() || context.entry_scope == c.scope)
                    {
                        return Err(format!(
                            "NeverIfTaggedCaveat: entry tagged '{}' is forbidden in scope '{}'",
                            c.tag, c.scope
                        ));
                    }
                }
                // OnlyIfTaggedCaveat
                4 => {
                    if !context.entry_tags.contains(&c.tag.as_str()) {
                        return Err(format!(
                            "OnlyIfTaggedCaveat: entry must carry tag '{}'",
                            c.tag
                        ));
                    }
                }
                _ => {}
            }
        }
        cap_proto::caveat::Spec::ConstitutionVersion(c) => {
            // Simple prefix match for now; full semver range in production.
            if !context.constitution_version.starts_with(&c.version_range) {
                return Err(format!(
                    "ConstitutionVersionCaveat: version '{}' does not match range '{}'",
                    context.constitution_version, c.version_range
                ));
            }
        }
    }

    Ok(())
}

/// Context passed to caveat evaluation.
pub struct CaveatContext<'a> {
    pub actions_in_window: i64,
    pub entry_tags: Vec<&'a str>,
    pub entry_scope: &'a str,
    pub constitution_version: &'a str,
    pub has_supervisor_countersign: bool,
}

/// Evaluate all caveats. Returns the first failure if any.
pub fn evaluate_all<'a>(
    caveats: &[cap_proto::Caveat],
    context: &CaveatContext<'a>,
) -> Result<(), Vec<String>> {
    let failures: Vec<String> = caveats
        .iter()
        .filter_map(|c| evaluate_caveat(c, context).err())
        .collect();

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}
