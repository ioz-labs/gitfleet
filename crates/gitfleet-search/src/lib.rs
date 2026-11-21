use gitfleet_core::{AppEvent, AppResult, EventBus, RepoId};
use ignore::WalkBuilder;
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub case_sensitive: bool,
    pub regex: bool,
    pub targets: Vec<(RepoId, String, PathBuf)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    pub repo_id: RepoId,
    pub repo_name: String,
    pub file_path: String,
    pub line_number: usize,
    pub line_preview: String,
}

/// Cross-repository Fleet Search engine backed by `ignore::WalkBuilder` + `regex`.
pub struct SearchService {
    events: EventBus,
    active_runs: RwLock<HashMap<String, Arc<AtomicBool>>>,
}

impl SearchService {
    pub fn new(events: EventBus) -> Self {
        Self {
            events,
            active_runs: RwLock::new(HashMap::new()),
        }
    }

    pub fn new_cancel_token(&self) -> (String, Arc<AtomicBool>) {
        let run_id = Uuid::new_v4().to_string();
        let flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut guard) = self.active_runs.write() {
            guard.insert(run_id.clone(), flag.clone());
        }
        (run_id, flag)
    }

    pub async fn execute(&self, request: SearchRequest) -> AppResult<(String, Vec<SearchMatch>)> {
        let (run_id, cancelled) = self.new_cancel_token();
        let events = self.events.clone();
        let rid = run_id.clone();

        let matches = tokio::task::spawn_blocking(move || {
            let pattern = if request.regex {
                request.query.clone()
            } else {
                regex::escape(&request.query)
            };
            let Ok(re) = RegexBuilder::new(&pattern)
                .case_insensitive(!request.case_sensitive)
                .build()
            else {
                return Vec::new();
            };

            let mut results = Vec::new();
            const MAX_MATCHES: usize = 500;

            'outer: for (repo_id, repo_name, root) in request.targets {
                if cancelled.load(Ordering::Relaxed) || !root.is_dir() {
                    break;
                }
                let walker = WalkBuilder::new(&root)
                    .hidden(false)
                    .git_ignore(true)
                    .git_exclude(true)
                    .max_filesize(Some(2 * 1024 * 1024))
                    .filter_entry(|e| {
                        e.file_name()
                            .to_str()
                            .is_none_or(|n| n != ".git" && n != "node_modules" && n != "target")
                    })
                    .build();

                for entry in walker.flatten() {
                    if cancelled.load(Ordering::Relaxed) {
                        break 'outer;
                    }
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let Ok(content) = std::fs::read_to_string(path) else {
                        continue;
                    };
                    let rel = path
                        .strip_prefix(&root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .replace('\\', "/");

                    for (idx, line) in content.lines().enumerate() {
                        if re.is_match(line) {
                            let preview: String = line.trim().chars().take(120).collect();
                            results.push(SearchMatch {
                                repo_id: repo_id.clone(),
                                repo_name: repo_name.clone(),
                                file_path: rel.clone(),
                                line_number: idx + 1,
                                line_preview: preview.clone(),
                            });
                            events.publish(AppEvent::SearchResult {
                                run_id: rid.clone(),
                                repo_id: repo_id.clone(),
                                file: rel.clone(),
                                line: idx + 1,
                                preview,
                            });
                            if results.len() >= MAX_MATCHES {
                                break 'outer;
                            }
                        }
                    }
                }
            }
            events.publish(AppEvent::SearchFinished {
                run_id: rid,
                total_matches: results.len(),
            });
            results
        })
        .await
        .unwrap_or_default();

        if let Ok(mut guard) = self.active_runs.write() {
            guard.remove(&run_id);
        }
        Ok((run_id, matches))
    }

    pub fn cancel(&self, run_id: &str) {
        if let Ok(mut guard) = self.active_runs.write()
            && let Some(flag) = guard.remove(run_id)
        {
            flag.store(true, Ordering::Relaxed);
        }
    }

    pub fn cancel_all(&self) {
        if let Ok(mut guard) = self.active_runs.write() {
            for (_, flag) in guard.drain() {
                flag.store(true, Ordering::Relaxed);
            }
        }
    }
}
