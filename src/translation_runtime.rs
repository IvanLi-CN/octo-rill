//! Translation worker concurrency defaults shared by the admin surfaces.
//!
//! The SQLite translation scheduler and the PostgreSQL admin read model report
//! the same configured defaults, so the constants live here rather than in the
//! retiring scheduler module.

pub(crate) const DEFAULT_TRANSLATION_GENERAL_WORKER_CONCURRENCY: usize = 3;
pub(crate) const DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY: usize = 1;
