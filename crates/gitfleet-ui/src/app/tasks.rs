use crate::app::{GitFleetApp, Message, PendingConfirm};
use gitfleet_app::AppService;
use gitfleet_git::LogOptions;
use gitfleet_security::IgnoreLayer;
use gitfleet_workspace::RepoEntry;
use iced::Task;
use iced::widget::text_editor;
use std::sync::Arc;

impl GitFleetApp {
    pub(crate) fn load_status_task(&self, repo: RepoEntry) -> Task<Message> {
        let svc = self.service.clone();
        Task::perform(
            async move {
                let res = svc
                    .git
                    .get_status(&repo.path)
                    .await
                    .map_err(|e| e.to_string());
                (repo.id, res)
            },
            |(id, res)| Message::StatusLoaded(id, res),
        )
    }

    pub(crate) fn load_log_task(&self, repo: RepoEntry) -> Task<Message> {
        let svc = self.service.clone();
        let all_refs = self.graph_all_branches;
        let topo_order = self.graph_topo_order;
        let selected_refs: Vec<String> = self.graph_checked_branches.iter().cloned().collect();
        Task::perform(
            async move {
                let res = svc
                    .git
                    .get_log(
                        &repo.path,
                        LogOptions {
                            limit: 160,
                            all_refs,
                            topo_order,
                            selected_refs,
                            ..Default::default()
                        },
                    )
                    .await
                    .map_err(|e| e.to_string());
                (repo.id, res)
            },
            |(id, res)| Message::LogLoaded(id, res),
        )
    }

    pub(crate) fn load_branches_task(&self, repo: RepoEntry) -> Task<Message> {
        let svc = self.service.clone();
        Task::perform(
            async move {
                let res = svc
                    .git
                    .get_branches(&repo.path)
                    .await
                    .map_err(|e| e.to_string());
                (repo.id, res)
            },
            |(id, res)| Message::BranchesLoaded(id, res),
        )
    }

    pub(crate) fn load_all_files_task(&self, repo: RepoEntry) -> Task<Message> {
        let svc = self.service.clone();
        Task::perform(
            async move {
                let res = svc
                    .git
                    .list_repo_files(&repo.path)
                    .await
                    .map_err(|e| e.to_string());
                (repo.id, res)
            },
            |(id, res)| Message::AllFilesLoaded(id, res),
        )
    }

    pub(crate) fn load_ignored_entries_task(&self, repo: RepoEntry) -> Task<Message> {
        let svc = self.service.clone();
        Task::perform(
            async move {
                let res = svc
                    .git
                    .list_ignored_entries(&repo.path)
                    .await
                    .map_err(|e| e.to_string());
                (repo.id, res)
            },
            |(id, res)| Message::IgnoredEntriesLoaded(id, res),
        )
    }

    pub(crate) fn load_ignored_dir_children_task(
        &self,
        repo: RepoEntry,
        rel_dir: String,
    ) -> Task<Message> {
        Task::perform(
            async move {
                let dir_clone = rel_dir.clone();
                let res = tokio::task::spawn_blocking(move || {
                    gitfleet_git::list_ignored_dir_children(&repo.path, &dir_clone, 500)
                        .map_err(|e| e.to_string())
                })
                .await
                .unwrap_or_else(|e| Err(format!("Failed to list directory: {e}")));
                (repo.id, rel_dir, res)
            },
            |(id, dir, res)| Message::IgnoredDirChildrenLoaded(id, dir, res),
        )
    }

    pub(crate) fn load_stashes_task(&self, repo: RepoEntry) -> Task<Message> {
        let svc = self.service.clone();
        Task::perform(
            async move {
                let res = svc
                    .git
                    .list_stashes(&repo.path)
                    .await
                    .map_err(|e| e.to_string());
                (repo.id, res)
            },
            |(id, res)| Message::StashesLoaded(id, res),
        )
    }

    pub(crate) fn load_tags_task(&self, repo: RepoEntry) -> Task<Message> {
        let svc = self.service.clone();
        Task::perform(
            async move {
                let res = svc
                    .git
                    .get_tags(&repo.path)
                    .await
                    .map_err(|e| e.to_string());
                (repo.id, res)
            },
            |(id, res)| Message::TagsLoaded(id, res),
        )
    }

    pub(crate) fn load_reflog_task(&self, repo: RepoEntry) -> Task<Message> {
        let svc = self.service.clone();
        Task::perform(
            async move {
                let res = svc
                    .git
                    .get_reflog(&repo.path, 60)
                    .await
                    .map_err(|e| e.to_string());
                (repo.id, res)
            },
            |(id, res)| Message::ReflogLoaded(id, res),
        )
    }

    pub(crate) fn load_submodules_task(&self, repo: RepoEntry) -> Task<Message> {
        let svc = self.service.clone();
        Task::perform(
            async move {
                let res = svc
                    .git
                    .get_submodules(&repo.path)
                    .await
                    .map_err(|e| e.to_string());
                (repo.id, res)
            },
            |(id, res)| Message::SubmodulesLoaded(id, res),
        )
    }

    pub(crate) fn load_worktrees_task(&self, repo: RepoEntry) -> Task<Message> {
        let svc = self.service.clone();
        Task::perform(
            async move {
                let res = svc
                    .git
                    .get_worktrees(&repo.path)
                    .await
                    .map_err(|e| e.to_string());
                (repo.id, res)
            },
            |(id, res)| Message::WorktreesLoaded(id, res),
        )
    }

    pub(crate) fn scan_hygiene_task(&self, repo: RepoEntry) -> Task<Message> {
        let svc = self.service.clone();
        let threshold_bytes = svc.settings().oversized_threshold_mb.max(1) * 1024 * 1024;
        Task::perform(
            async move {
                let diff = svc.git.staged_diff(&repo.path).await.unwrap_or_default();
                let paths = svc
                    .git
                    .staged_file_paths(&repo.path)
                    .await
                    .unwrap_or_default();
                let secrets = svc.security.scan_staged_diff_secrets(&diff);
                let oversized = svc
                    .security
                    .scan_oversized_staged(&repo.path, &paths, threshold_bytes);
                (repo.id, secrets, oversized)
            },
            |(id, s, o)| Message::HygieneScanned(id, s, o),
        )
    }

    /// Lightweight refresh of `vault_summary` only (preserves `ignore_editor` and revealed secrets)
    /// so external edits to `.env` / `.toml` / `.yaml` immediately surface Modified indicators.
    pub(crate) fn refresh_vault_summary(&mut self) {
        let Some(repo) = self.current_repo() else {
            self.vault_summary = gitfleet_security::VaultSummary::default();
            return;
        };
        let preferred_key = self
            .active_repo_secret_key_binding()
            .map(|(key_id, _)| key_id);
        let active_profile = self.active_repo_secret_profile();
        self.vault_summary = self
            .service
            .security
            .vault_summary_for_profile(
                &repo.path,
                active_profile.as_deref(),
                preferred_key.as_deref(),
            )
            .unwrap_or_default();
        if let Some(first) = self.vault_summary.files.first()
            && (self.secret_file_input.trim().is_empty()
                || !self
                    .vault_summary
                    .files
                    .iter()
                    .any(|f| f.path == self.secret_file_input))
        {
            self.secret_file_input = first.path.clone();
        }
    }

    pub(crate) fn refresh_vault_and_ignore(&mut self) {
        let Some(repo) = self.current_repo() else {
            return;
        };
        self.refresh_vault_summary();
        self.secret_candidates = self
            .service
            .security
            .discover_candidate_files(&repo.path);
        self.revealed_secrets.clear();
        if let Ok(snap) = self.service.security.ignore_snapshot(&repo.path) {
            let text = match self.ignore_layer {
                IgnoreLayer::RootGitignore => snap.root_gitignore.clone(),
                IgnoreLayer::PersonalExclude => snap.personal_exclude.clone(),
                IgnoreLayer::GlobalExclude => snap.global_exclude.clone(),
            };
            self.ignore_snapshot = snap;
            self.ignore_editor = text_editor::Content::with_text(&text);
        }
    }

    pub(crate) fn try_commit(&mut self, push_after: bool) -> Task<Message> {
        let raw = self.composed_commit_message();
        let msg = raw.trim();
        if msg.is_empty() {
            self.report_action_error(
                "Commit Failed",
                "Commit message cannot be empty. Enter a commit message before committing.",
                false,
            );
            return Task::none();
        }
        if !self.amend_commit
            && let Some(st) = self.active_status()
            && st.staged_count == 0
        {
            if st.unstaged_count > 0 {
                let preview: Vec<String> = st
                    .unstaged_files
                    .iter()
                    .take(6)
                    .map(|f| f.path.clone())
                    .collect();
                self.pending_confirm = Some(PendingConfirm::commit_all_working_tree(
                    st.unstaged_count,
                    &preview,
                    push_after,
                ));
                return Task::none();
            }
            self.report_action_error(
                "Nothing to Commit",
                "Working tree is clean — there are no staged or modified changes to commit.",
                false,
            );
            return Task::none();
        }
        if !self.staged_secrets.is_empty() {
            self.pending_confirm = Some(PendingConfirm::commit_with_secrets(
                self.staged_secrets.len(),
                push_after,
            ));
            return Task::none();
        }
        if self.amend_commit
            && let Some(target) = &self.amend_target
            && target.pushed_to_upstream
        {
            self.pending_confirm = Some(PendingConfirm::amend_pushed(&target.short_oid, push_after));
            return Task::none();
        }
        self.execute_commit(push_after)
    }

    pub(crate) fn execute_commit_all(&mut self, push_after: bool) -> Task<Message> {
        let raw = self.composed_commit_message();
        let msg = raw.trim().replace("\r\n", "\n");
        if msg.is_empty() {
            self.report_action_error(
                "Commit Failed",
                "Commit message cannot be empty. Enter a commit message before committing.",
                false,
            );
            return Task::none();
        }
        let amend = self.amend_commit;
        self.clear_commit_composer();
        self.amend_commit = false;
        self.amend_target = None;
        let label = if push_after {
            "Commit All & Push"
        } else {
            "Commit All"
        };
        self.run_git_op(label, move |svc, repo| async move {
            svc.git
                .commit_all_changes(&repo.path, &msg, amend, push_after)
                .await
        })
    }

    pub(crate) fn execute_commit(&mut self, push_after: bool) -> Task<Message> {
        let raw = self.composed_commit_message();
        let msg = raw.trim().replace("\r\n", "\n");
        if msg.is_empty() {
            self.report_action_error(
                "Commit Failed",
                "Commit message cannot be empty. Enter a commit message before committing.",
                false,
            );
            return Task::none();
        }
        let amend = self.amend_commit;
        self.clear_commit_composer();
        self.amend_commit = false;
        self.amend_target = None;
        if push_after {
            self.run_git_op("Commit & Push", move |svc, repo| async move {
                svc.git.commit_and_push(&repo.path, &msg, amend).await
            })
        } else {
            self.run_git_op("Commit", move |svc, repo| async move {
                svc.git.commit(&repo.path, &msg, amend).await
            })
        }
    }

    pub(crate) fn run_git_op<F, Fut>(&mut self, label: &'static str, op: F) -> Task<Message>
    where
        F: FnOnce(Arc<AppService>, RepoEntry) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = gitfleet_core::AppResult<String>> + Send + 'static,
    {
        let Some(repo) = self.current_repo() else {
            return Task::none();
        };
        self.busy_label = Some(format!("{label}…"));
        let svc = self.service.clone();
        Task::perform(
            async move {
                let res = op(svc, repo).await.map_err(|e| e.to_string());
                (label.to_string(), res)
            },
            |(l, r)| Message::GitActionDone(l, r),
        )
    }
}
