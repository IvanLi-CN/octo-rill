//! Task-type taxonomy shared by the PostgreSQL admin job surfaces.
//!
//! The PostgreSQL runtime replaced SQLite on 2026-11-21. Imported SQLite rows
//! still live in `public.job_tasks` and use task types the current runtime never
//! produces, so every admin read surface needs one classification instead of the
//! legacy hardcoded lists.
//!
//! [`ADMIN_JOB_TASK_GROUPS`] is the single source of truth: the dashboard
//! rollups, the job center `task_group` filter and the web labels all derive
//! from it. [`LEGACY_TASK_TYPES`] lists imported-only task types; they stay
//! readable in history but never count as current work.

use crate::jobs;

/// Task types outside the executor registry: PostgreSQL routes write them as
/// tracking rows, so the registry in `postgres_server` never claims them.
pub(crate) const TASK_TRANSLATION_GENERATE: &str = "translation.generate";
pub(crate) const TASK_SYNC_REPO_RELEASE: &str = "sync.repo_release";
pub(crate) const TASK_WEBHOOK_PUSH_RECONCILE: &str = "webhook.push.reconcile";

/// Executors registered by `serve_postgres`. The unit tests below require every
/// entry to appear in exactly one [`ADMIN_JOB_TASK_GROUPS`] group.
pub(crate) const POSTGRES_TASK_EXECUTOR_TYPES: &[&str] = &[
    crate::postgres_tasks::TASK_SYNC_STARRED,
    crate::postgres_tasks::TASK_SYNC_RELEASES,
    TASK_SYNC_REPO_RELEASE,
    crate::postgres_tasks::TASK_SYNC_NOTIFICATIONS,
    jobs::TASK_SYNC_ALL,
    jobs::TASK_SYNC_ACCESS_REFRESH,
    jobs::TASK_WEBHOOK_PUSH_MANAGE,
    TASK_WEBHOOK_PUSH_RECONCILE,
    jobs::TASK_BRIEF_GENERATE,
    jobs::TASK_BRIEF_DAILY_SLOT,
    TASK_TRANSLATION_GENERATE,
    crate::postgres_tasks::TASK_POLISH_BACKFILL,
];

/// Imported SQLite task types the PostgreSQL runtime never writes.
///
/// They remain in history so the job center can explain past failures, but the
/// overview counters and dashboard rollups exclude them.
pub(crate) const LEGACY_TASK_TYPES: &[&str] = &[
    crate::postgres_tasks::TASK_SYNC_SUBSCRIPTIONS,
    jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH,
    jobs::TASK_RELEASE_EVIDENCE,
    crate::jobs::TASK_SYNC_STARRED_DELTA,
    crate::jobs::TASK_SYNC_STARRED_RECONCILE,
    jobs::TASK_BRIEF_HISTORY_RECOMPUTE,
    jobs::TASK_BRIEF_REFRESH_CONTENT,
    jobs::TASK_WEBHOOK_PUSH_AUDIT,
];

/// How a task type enters the queue. The job center exposes these as the
/// `scheduled`, `user_sync` and `realtime` list tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdminJobTaskLane {
    Scheduled,
    UserSync,
    Realtime,
}

#[derive(Debug)]
pub(crate) struct AdminJobTaskTypeSpec {
    pub(crate) task_type: &'static str,
    pub(crate) label: &'static str,
    pub(crate) lane: AdminJobTaskLane,
    /// Parameter form the job center renders before a manual trigger.
    pub(crate) trigger_params: &'static [AdminJobTriggerParam],
}

/// One field of a manual-trigger form.
///
/// The declaration is the contract: the server validates the submitted value
/// against the same bounds the form shows, so a form and its validation cannot
/// drift apart the way the frontend task-type lists did.
#[derive(Debug)]
pub(crate) struct AdminJobTriggerParam {
    pub(crate) name: &'static str,
    pub(crate) label: &'static str,
    pub(crate) kind: AdminJobTriggerParamKind,
}

#[derive(Debug)]
pub(crate) enum AdminJobTriggerParamKind {
    /// Bounded integer. Both ends are inclusive.
    Integer { min: i64, max: i64, default: i64 },
}

#[derive(Debug)]
pub(crate) struct AdminJobTaskGroup {
    pub(crate) key: &'static str,
    pub(crate) label: &'static str,
    pub(crate) task_types: &'static [AdminJobTaskTypeSpec],
}

const fn spec(
    task_type: &'static str,
    label: &'static str,
    lane: AdminJobTaskLane,
) -> AdminJobTaskTypeSpec {
    spec_with_params(task_type, label, lane, &[])
}

/// Declare a task type that an operator may also trigger by hand.
///
/// `NO_TRIGGER_PARAMS` is the common case: the task takes no input, so the job
/// center renders a confirmation instead of a form.
const fn spec_with_params(
    task_type: &'static str,
    label: &'static str,
    lane: AdminJobTaskLane,
    trigger_params: &'static [AdminJobTriggerParam],
) -> AdminJobTaskTypeSpec {
    AdminJobTaskTypeSpec {
        task_type,
        label,
        lane,
        trigger_params,
    }
}

const fn int_param(
    name: &'static str,
    label: &'static str,
    min: i64,
    max: i64,
    default: i64,
) -> AdminJobTriggerParam {
    AdminJobTriggerParam {
        name,
        label,
        kind: AdminJobTriggerParamKind::Integer { min, max, default },
    }
}

/// Page bound for one manual `polish.backfill` trigger.
///
/// The scheduled lane advances one small page per hour; a hand trigger is the
/// operator asking for progress now, so it may cover more Releases in one run.
/// The ceiling keeps a single click from admitting a burst the provider cannot
/// absorb: each Release expands into up to three audience variants.
const POLISH_BACKFILL_TRIGGER_MAX_PAGE: i64 = 100;
const POLISH_BACKFILL_TRIGGER_DEFAULT_PAGE: i64 = 25;

const POLISH_BACKFILL_TRIGGER_PARAMS: &[AdminJobTriggerParam] = &[int_param(
    "page_size",
    "单次扫描 Release 数",
    POLISH_BACKFILL_TRIGGER_DEFAULT_PAGE,
    POLISH_BACKFILL_TRIGGER_MAX_PAGE,
    POLISH_BACKFILL_TRIGGER_DEFAULT_PAGE,
)];

/// Group -> label -> task types for the current PostgreSQL runtime.
///
/// `translate.release*` and `brief.*` tracking rows are written by the
/// PostgreSQL runtime itself (streaming translation and the brief executors),
/// so they are current even though no worker executor claims them.
pub(crate) const ADMIN_JOB_TASK_GROUPS: &[AdminJobTaskGroup] = &[
    AdminJobTaskGroup {
        key: "translation",
        label: "翻译",
        task_types: &[
            spec(
                TASK_TRANSLATION_GENERATE,
                "翻译生成",
                AdminJobTaskLane::Realtime,
            ),
            spec_with_params(
                crate::postgres_tasks::TASK_POLISH_BACKFILL,
                "润色受众变体回填",
                AdminJobTaskLane::Scheduled,
                POLISH_BACKFILL_TRIGGER_PARAMS,
            ),
            spec(
                jobs::TASK_TRANSLATE_RELEASE_BATCH,
                "批量翻译 Release",
                AdminJobTaskLane::Realtime,
            ),
            spec(
                jobs::TASK_TRANSLATE_RELEASE,
                "翻译 Release",
                AdminJobTaskLane::Realtime,
            ),
            spec(
                jobs::TASK_TRANSLATE_RELEASE_DETAIL,
                "翻译 Release 详情",
                AdminJobTaskLane::Realtime,
            ),
            spec(
                jobs::TASK_TRANSLATE_NOTIFICATION,
                "翻译通知",
                AdminJobTaskLane::Realtime,
            ),
        ],
    },
    AdminJobTaskGroup {
        key: "sync",
        label: "同步",
        task_types: &[
            spec(
                crate::postgres_tasks::TASK_SYNC_STARRED,
                "同步 Star",
                AdminJobTaskLane::UserSync,
            ),
            spec(
                crate::postgres_tasks::TASK_SYNC_RELEASES,
                "同步 Release",
                AdminJobTaskLane::Scheduled,
            ),
            spec(
                crate::postgres_tasks::TASK_SYNC_NOTIFICATIONS,
                "同步通知",
                AdminJobTaskLane::Scheduled,
            ),
            spec(
                TASK_SYNC_REPO_RELEASE,
                "同步仓库 Release",
                AdminJobTaskLane::Realtime,
            ),
            spec(jobs::TASK_SYNC_ALL, "全量同步", AdminJobTaskLane::Realtime),
            spec(
                jobs::TASK_SYNC_ACCESS_REFRESH,
                "访问增量同步",
                AdminJobTaskLane::Realtime,
            ),
        ],
    },
    AdminJobTaskGroup {
        key: "briefs",
        label: "日报",
        task_types: &[
            spec(
                jobs::TASK_BRIEF_DAILY_SLOT,
                "定时日报",
                AdminJobTaskLane::Scheduled,
            ),
            spec(
                jobs::TASK_BRIEF_GENERATE,
                "日报生成",
                AdminJobTaskLane::Scheduled,
            ),
        ],
    },
    AdminJobTaskGroup {
        key: "operations",
        label: "运维",
        task_types: &[
            spec(
                jobs::TASK_WEBHOOK_PUSH_MANAGE,
                "Webhook 管理",
                AdminJobTaskLane::Scheduled,
            ),
            spec(
                TASK_WEBHOOK_PUSH_RECONCILE,
                "Webhook 对账",
                AdminJobTaskLane::Realtime,
            ),
            spec(
                jobs::TASK_RETRY_RECENT_FAILURES,
                "失败数据重试",
                AdminJobTaskLane::Scheduled,
            ),
            spec(
                jobs::TASK_RELEASE_READING_BACKFILL,
                "阅读回填",
                AdminJobTaskLane::Scheduled,
            ),
        ],
    },
];

pub(crate) fn runtime_task_specs() -> impl Iterator<Item = &'static AdminJobTaskTypeSpec> {
    ADMIN_JOB_TASK_GROUPS
        .iter()
        .flat_map(|group| group.task_types.iter())
}

pub(crate) fn runtime_task_types() -> impl Iterator<Item = &'static str> {
    runtime_task_specs().map(|spec| spec.task_type)
}

pub(crate) fn find_group(key: &str) -> Option<&'static AdminJobTaskGroup> {
    ADMIN_JOB_TASK_GROUPS.iter().find(|group| group.key == key)
}

pub(crate) fn group_for_task_type(task_type: &str) -> Option<&'static AdminJobTaskGroup> {
    ADMIN_JOB_TASK_GROUPS.iter().find(|group| {
        group
            .task_types
            .iter()
            .any(|spec| spec.task_type == task_type)
    })
}

pub(crate) fn lane_task_types(lane: AdminJobTaskLane) -> impl Iterator<Item = &'static str> {
    runtime_task_specs()
        .filter(move |spec| spec.lane == lane)
        .map(|spec| spec.task_type)
}

/// Task types of one group that the periodic lanes dispatch.
pub(crate) fn scheduled_group_task_types(group_key: &str) -> Vec<&'static str> {
    find_group(group_key)
        .map(|group| {
            group
                .task_types
                .iter()
                .filter(|spec| spec.lane == AdminJobTaskLane::Scheduled)
                .map(|spec| spec.task_type)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn is_legacy_task_type(task_type: &str) -> bool {
    LEGACY_TASK_TYPES.contains(&task_type)
}

/// Task types an operator may start by hand.
///
/// Kept as an explicit allowlist rather than "any type with a form": a global
/// sync reachable from one misplaced click costs far more than the convenience
/// of exposing every type.
pub(crate) const TRIGGERABLE_TASK_TYPES: &[&str] = &[crate::postgres_tasks::TASK_POLISH_BACKFILL];

pub(crate) fn task_type_is_triggerable(task_type: &str) -> bool {
    TRIGGERABLE_TASK_TYPES.contains(&task_type)
}

/// Page bound a `polish.backfill` task should use.
///
/// Reads the validated `page_size` the trigger handler wrote into the task
/// payload, falling back to the scheduled page when absent or out of range. The
/// clamp lives here as well as in validation so a row written by anything other
/// than the trigger handler cannot widen the bound.
pub(crate) fn polish_backfill_page_from_payload_json(payload_json: Option<&str>) -> usize {
    let parsed: serde_json::Value = payload_json
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or(serde_json::Value::Null);
    let requested = parsed
        .get("params")
        .and_then(|params| params.get("page_size"))
        .and_then(serde_json::Value::as_i64);
    let resolved = requested.unwrap_or(POLISH_BACKFILL_TRIGGER_DEFAULT_PAGE);
    let clamped = resolved.clamp(
        POLISH_BACKFILL_TRIGGER_DEFAULT_PAGE,
        POLISH_BACKFILL_TRIGGER_MAX_PAGE,
    );
    usize::try_from(clamped).unwrap_or(crate::postgres_content::POLISH_ADMISSION_CAP)
}

/// Validate a submitted trigger form against the type's declaration.
///
/// Returns the resolved parameters (defaults applied) or a message naming the
/// offending field. Unknown keys are rejected so a stale form cannot silently
/// submit a parameter the server would ignore.
pub(crate) fn validate_trigger_params(
    spec: &AdminJobTaskTypeSpec,
    submitted: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let empty = serde_json::Map::new();
    let submitted = submitted.unwrap_or(&empty);
    for key in submitted.keys() {
        if !spec.trigger_params.iter().any(|param| param.name == key) {
            return Err(format!("unknown parameter: {key}"));
        }
    }
    let mut resolved = serde_json::Map::new();
    for param in spec.trigger_params {
        let AdminJobTriggerParamKind::Integer { min, max, default } = &param.kind;
        let value = match submitted.get(param.name) {
            None => *default,
            Some(raw) => raw
                .as_i64()
                .ok_or_else(|| format!("{} must be an integer", param.name))?,
        };
        if value < *min || value > *max {
            return Err(format!("{} must be between {min} and {max}", param.name));
        }
        resolved.insert(param.name.to_owned(), serde_json::Value::from(value));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_params_apply_defaults_and_clamp() {
        let spec = runtime_task_specs()
            .find(|spec| spec.task_type == crate::postgres_tasks::TASK_POLISH_BACKFILL)
            .expect("polish backfill spec");
        let resolved = validate_trigger_params(spec, None).expect("defaults accepted");
        assert_eq!(resolved.get("page_size").and_then(|v| v.as_i64()), Some(25));

        let mut submitted = serde_json::Map::new();
        submitted.insert("page_size".to_owned(), serde_json::Value::from(100));
        let resolved = validate_trigger_params(spec, Some(&submitted)).expect("max accepted");
        assert_eq!(
            resolved.get("page_size").and_then(|v| v.as_i64()),
            Some(100)
        );

        submitted.insert("page_size".to_owned(), serde_json::Value::from(101));
        assert!(validate_trigger_params(spec, Some(&submitted)).is_err());

        submitted.insert("page_size".to_owned(), serde_json::Value::from(0));
        assert!(validate_trigger_params(spec, Some(&submitted)).is_err());

        let mut unknown = serde_json::Map::new();
        unknown.insert("nope".to_owned(), serde_json::Value::from(1));
        assert!(validate_trigger_params(spec, Some(&unknown)).is_err());
    }

    #[test]
    fn only_declared_types_are_triggerable() {
        assert!(task_type_is_triggerable(
            crate::postgres_tasks::TASK_POLISH_BACKFILL
        ));
        assert!(!task_type_is_triggerable(
            crate::postgres_tasks::TASK_SYNC_RELEASES
        ));
        for task_type in TRIGGERABLE_TASK_TYPES {
            assert!(
                runtime_task_specs().any(|spec| spec.task_type == *task_type),
                "{task_type} is triggerable but not a runtime task type"
            );
        }
    }

    #[test]
    fn backfill_page_falls_back_and_clamps() {
        assert_eq!(polish_backfill_page_from_payload_json(None), 25);
        assert_eq!(
            polish_backfill_page_from_payload_json(Some(r#"{"params":{"page_size":5000}}"#)),
            100
        );
        assert_eq!(
            polish_backfill_page_from_payload_json(Some(r#"{"params":{"page_size":1}}"#)),
            25
        );
        assert_eq!(
            polish_backfill_page_from_payload_json(Some(r#"{"params":{"page_size":80}}"#)),
            80
        );
        assert_eq!(polish_backfill_page_from_payload_json(Some("not json")), 25);
    }

    #[test]
    fn registered_executors_appear_in_exactly_one_group() {
        for task_type in POSTGRES_TASK_EXECUTOR_TYPES {
            let matches = ADMIN_JOB_TASK_GROUPS
                .iter()
                .filter(|group| {
                    group
                        .task_types
                        .iter()
                        .any(|spec| spec.task_type == *task_type)
                })
                .count();
            assert_eq!(matches, 1, "{task_type} must belong to exactly one group");
        }
    }

    #[test]
    fn runtime_task_types_are_unique_and_not_legacy() {
        let mut seen = Vec::new();
        for task_type in runtime_task_types() {
            assert!(
                !seen.contains(&task_type),
                "{task_type} appears in more than one group"
            );
            assert!(
                !is_legacy_task_type(task_type),
                "{task_type} is both current and legacy"
            );
            seen.push(task_type);
        }
    }

    #[test]
    fn groups_cover_translation_sync_briefs_and_operations() {
        let keys = ADMIN_JOB_TASK_GROUPS
            .iter()
            .map(|group| group.key)
            .collect::<Vec<_>>();
        assert_eq!(keys, vec!["translation", "sync", "briefs", "operations"]);
        for group in ADMIN_JOB_TASK_GROUPS {
            assert!(!group.task_types.is_empty(), "group {} is empty", group.key);
        }
        assert!(runtime_task_types().any(|task_type| task_type == TASK_TRANSLATION_GENERATE));
        assert!(
            !runtime_task_types()
                .any(|task_type| task_type == crate::postgres_tasks::TASK_SYNC_SUBSCRIPTIONS)
        );
    }

    #[test]
    fn legacy_types_are_outside_every_group_and_lane() {
        for legacy in LEGACY_TASK_TYPES {
            assert!(
                group_for_task_type(legacy).is_none(),
                "{legacy} must not be a runtime task type"
            );
            assert!(is_legacy_task_type(legacy));
        }
        assert!(is_legacy_task_type(
            crate::postgres_tasks::TASK_SYNC_SUBSCRIPTIONS
        ));
        assert!(!is_legacy_task_type(TASK_TRANSLATION_GENERATE));
        assert!(!is_legacy_task_type("unknown.task.type"));
    }

    #[test]
    fn lanes_partition_the_runtime_task_types() {
        let scheduled = lane_task_types(AdminJobTaskLane::Scheduled).collect::<Vec<_>>();
        let user_sync = lane_task_types(AdminJobTaskLane::UserSync).collect::<Vec<_>>();
        let realtime = lane_task_types(AdminJobTaskLane::Realtime).collect::<Vec<_>>();
        assert!(scheduled.contains(&jobs::TASK_BRIEF_DAILY_SLOT));
        assert!(scheduled.contains(&jobs::TASK_BRIEF_GENERATE));
        assert!(!scheduled.contains(&crate::postgres_tasks::TASK_SYNC_STARRED));
        assert_eq!(user_sync, vec![crate::postgres_tasks::TASK_SYNC_STARRED]);
        assert!(realtime.contains(&TASK_TRANSLATION_GENERATE));
        assert!(realtime.contains(&TASK_SYNC_REPO_RELEASE));
        assert_eq!(
            scheduled.len() + user_sync.len() + realtime.len(),
            runtime_task_specs().count()
        );
    }

    #[test]
    fn scheduled_sync_lane_uses_current_release_and_notification_types() {
        let sync = scheduled_group_task_types("sync");
        assert_eq!(
            sync,
            vec![
                crate::postgres_tasks::TASK_SYNC_RELEASES,
                crate::postgres_tasks::TASK_SYNC_NOTIFICATIONS
            ]
        );
        assert!(!sync.contains(&crate::postgres_tasks::TASK_SYNC_SUBSCRIPTIONS));
        assert!(!sync.contains(&crate::postgres_tasks::TASK_SYNC_STARRED));
        assert!(scheduled_group_task_types("unknown").is_empty());
    }
}
