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
    jobs::TASK_SYNC_STARRED,
    jobs::TASK_SYNC_RELEASES,
    TASK_SYNC_REPO_RELEASE,
    jobs::TASK_SYNC_NOTIFICATIONS,
    jobs::TASK_SYNC_ALL,
    jobs::TASK_SYNC_ACCESS_REFRESH,
    jobs::TASK_WEBHOOK_PUSH_MANAGE,
    TASK_WEBHOOK_PUSH_RECONCILE,
    jobs::TASK_BRIEF_GENERATE,
    jobs::TASK_BRIEF_DAILY_SLOT,
    TASK_TRANSLATION_GENERATE,
];

/// Imported SQLite task types the PostgreSQL runtime never writes.
///
/// They remain in history so the job center can explain past failures, but the
/// overview counters and dashboard rollups exclude them.
pub(crate) const LEGACY_TASK_TYPES: &[&str] = &[
    jobs::TASK_SYNC_SUBSCRIPTIONS,
    jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH,
    jobs::TASK_RELEASE_EVIDENCE,
    jobs::TASK_SYNC_STARRED_DELTA,
    jobs::TASK_SYNC_STARRED_RECONCILE,
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
    AdminJobTaskTypeSpec {
        task_type,
        label,
        lane,
    }
}

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
                jobs::TASK_SYNC_STARRED,
                "同步 Star",
                AdminJobTaskLane::UserSync,
            ),
            spec(
                jobs::TASK_SYNC_RELEASES,
                "同步 Release",
                AdminJobTaskLane::Scheduled,
            ),
            spec(
                jobs::TASK_SYNC_NOTIFICATIONS,
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(!runtime_task_types().any(|task_type| task_type == jobs::TASK_SYNC_SUBSCRIPTIONS));
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
        assert!(is_legacy_task_type(jobs::TASK_SYNC_SUBSCRIPTIONS));
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
        assert!(!scheduled.contains(&jobs::TASK_SYNC_STARRED));
        assert_eq!(user_sync, vec![jobs::TASK_SYNC_STARRED]);
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
            vec![jobs::TASK_SYNC_RELEASES, jobs::TASK_SYNC_NOTIFICATIONS]
        );
        assert!(!sync.contains(&jobs::TASK_SYNC_SUBSCRIPTIONS));
        assert!(!sync.contains(&jobs::TASK_SYNC_STARRED));
        assert!(scheduled_group_task_types("unknown").is_empty());
    }
}
