use crate::{GitService, runner};
use gitfleet_core::{AppEvent, AppResult, RepoId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::Semaphore;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum BulkOperation {
    Fetch,
    Pull,
    Push,
    StatusRefresh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkItemResult {
    pub repo_id: RepoId,
    pub ok: bool,
    pub message: String,
}

impl GitService {
    pub async fn bulk_execute(
        &self,
        operation: BulkOperation,
        targets: Vec<(RepoId, PathBuf)>,
        max_parallel: usize,
    ) -> Vec<BulkItemResult> {
        let sem = Arc::new(Semaphore::new(max_parallel.max(1)));
        let mut handles = Vec::with_capacity(targets.len());

        for (id, path) in targets {
            let permit = sem.clone();
            let log = self.command_log.clone();
            let events = self.events.clone();
            handles.push(tokio::spawn(async move {
                let _guard = permit.acquire_owned().await;
                let res = match operation {
                    BulkOperation::Fetch => {
                        runner::run_git_ok(
                            &path,
                            &["fetch", "--all", "--prune"],
                            Some(&log),
                            Some(&events),
                            true,
                        )
                        .await
                    }
                    BulkOperation::Pull => {
                        runner::run_git_ok(
                            &path,
                            &["pull", "--ff-only"],
                            Some(&log),
                            Some(&events),
                            true,
                        )
                        .await
                    }
                    BulkOperation::Push => {
                        runner::run_git_ok(&path, &["push"], Some(&log), Some(&events), true).await
                    }
                    BulkOperation::StatusRefresh => {
                        runner::run_git_ok(
                            &path,
                            &["status", "--porcelain=v2", "--branch"],
                            Some(&log),
                            Some(&events),
                            false,
                        )
                        .await
                    }
                };
                match res {
                    Ok(_) => BulkItemResult {
                        repo_id: id,
                        ok: true,
                        message: "OK".into(),
                    },
                    Err(e) => BulkItemResult {
                        repo_id: id,
                        ok: false,
                        message: e.to_string(),
                    },
                }
            }));
        }

        let mut results = Vec::new();
        for h in handles {
            if let Ok(item) = h.await {
                results.push(item);
            }
        }
        results
    }

    pub async fn bulk_execute_cancellable(
        &self,
        operation: BulkOperation,
        targets: Vec<(RepoId, PathBuf)>,
        max_parallel: usize,
        cancel_flag: Arc<AtomicBool>,
    ) -> Vec<BulkItemResult> {
        let total = targets.len().max(1);
        let done = Arc::new(AtomicUsize::new(0));
        let sem = Arc::new(Semaphore::new(max_parallel.max(1)));
        let mut handles = Vec::with_capacity(targets.len());

        for (id, path) in targets {
            let permit = sem.clone();
            let log = self.command_log.clone();
            let events = self.events.clone();
            let cancel = cancel_flag.clone();
            let done_c = done.clone();
            handles.push(tokio::spawn(async move {
                let _guard = permit.acquire_owned().await;
                if cancel.load(Ordering::Relaxed) {
                    return BulkItemResult {
                        repo_id: id,
                        ok: false,
                        message: "Cancelled".into(),
                    };
                }
                let res = match operation {
                    BulkOperation::Fetch => {
                        runner::run_git_ok(
                            &path,
                            &["fetch", "--all", "--prune"],
                            Some(&log),
                            Some(&events),
                            true,
                        )
                        .await
                    }
                    BulkOperation::Pull => {
                        runner::run_git_ok(
                            &path,
                            &["pull", "--ff-only"],
                            Some(&log),
                            Some(&events),
                            true,
                        )
                        .await
                    }
                    BulkOperation::Push => {
                        runner::run_git_ok(&path, &["push"], Some(&log), Some(&events), true).await
                    }
                    BulkOperation::StatusRefresh => {
                        runner::run_git_ok(
                            &path,
                            &["status", "--porcelain=v2", "--branch"],
                            Some(&log),
                            Some(&events),
                            false,
                        )
                        .await
                    }
                };
                let cur = done_c.fetch_add(1, Ordering::Relaxed) + 1;
                events.publish(AppEvent::OperationProgress {
                    token: "bulk".into(),
                    percent: ((cur * 100) / total).min(100) as u8,
                    message: format!("{cur}/{total}"),
                });
                match res {
                    Ok(_) => BulkItemResult {
                        repo_id: id,
                        ok: true,
                        message: "OK".into(),
                    },
                    Err(e) => BulkItemResult {
                        repo_id: id,
                        ok: false,
                        message: e.to_string(),
                    },
                }
            }));
        }

        let mut results = Vec::new();
        for h in handles {
            if let Ok(item) = h.await {
                results.push(item);
            }
        }
        results
    }

    pub async fn run_bulk(
        &self,
        operation: BulkOperation,
        repo_ids: &[RepoId],
    ) -> AppResult<Vec<BulkItemResult>> {
        self.events.publish(AppEvent::StatusBatchUpdated {
            repo_ids: repo_ids.to_vec(),
        });
        let _ = operation;
        Ok(Vec::new())
    }
}
