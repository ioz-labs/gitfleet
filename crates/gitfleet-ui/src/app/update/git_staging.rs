use crate::app::{ActiveFocusPane, GitFleetApp, GraphSelection, GraphTab, Message};
use iced::Task;
use iced::widget::text_editor;

impl GitFleetApp {
    pub(crate) fn try_update_git_staging(&mut self, message: Message) -> Result<Task<Message>, Message> {
        let mut unhandled: Option<Message> = None;
        let task = (|| -> Task<Message> {
            match message {
            Message::RefreshSelected => {
                if let Some(repo) = self.current_repo() {
                    self.busy_label = Some("Refreshing…".into());
                    self.release_triggers = self.service.git.read_release_triggers(&repo.path);
                    let mut tasks = vec![
                        self.load_status_task(repo.clone()),
                        self.load_log_task(repo.clone()),
                        self.load_branches_task(repo.clone()),
                        self.load_stashes_task(repo.clone()),
                        self.load_tags_task(repo.clone()),
                        self.load_submodules_task(repo.clone()),
                        self.load_worktrees_task(repo.clone()),
                        self.load_all_files_task(repo.clone()),
                    ];
                    if self.graph_tab == GraphTab::Reflog {
                        tasks.push(self.load_reflog_task(repo));
                    }
                    if let Some((parent_id, _)) = &self.active_submodule
                        && let Some(parent) = self
                            .service
                            .workspace
                            .current()
                            .repos
                            .into_iter()
                            .find(|r| &r.id == parent_id)
                    {
                        tasks.push(self.load_status_task(parent.clone()));
                        tasks.push(self.load_submodules_task(parent));
                    }
                    return Task::batch(tasks);
                }
                Task::none()
            }
            Message::RefreshAll => {
                let repos = self.service.workspace.current().repos;
                let count = repos.len();
                let mut tasks = Vec::with_capacity(count * 2);
                for r in repos {
                    tasks.push(self.load_status_task(r.clone()));
                    tasks.push(self.load_submodules_task(r));
                }
                self.toast_is_error = false;
                self.toast_message = format!("Refreshing {count} repositories…");
                Task::batch(tasks)
            }
            Message::StatusLoaded(id, res) => {
                self.busy_label = None;
                match res {
                    Ok(summary) => {
                        let has_staged = summary.staged_count > 0;
                        let is_selected = self.active_repo_key().as_ref() == Some(&id);
                        self.statuses.insert(id, summary);
                        if is_selected {
                            self.prune_file_selection();
                            if has_staged && let Some(repo) = self.current_repo() {
                                return self.scan_hygiene_task(repo);
                            } else {
                                self.staged_secrets.clear();
                                self.oversized_files.clear();
                            }
                        }
                    }
                    Err(e) => {
                        self.toast_is_error = true;
                        self.toast_message = format!("Status error: {e}");
                    }
                }
                Task::none()
            }
            Message::StageFile(path) => self.run_git_op("Stage", move |svc, repo| async move {
                svc.git.stage(&repo.path, &[path]).await.map(|_| "Staged".into())
            }),
            Message::UnstageFile(path) => self.run_git_op("Unstage", move |svc, repo| async move {
                svc.git
                    .unstage(&repo.path, &[path])
                    .await
                    .map(|_| "Unstaged".into())
            }),
            Message::StageSelectedFiles => {
                if self.has_modal_or_menu_open()
                    || self.graph_selection != GraphSelection::WorkingTree
                {
                    return Task::none();
                }
                let paths = self.selected_unstaged_paths();
                if paths.is_empty() {
                    self.toast_is_error = false;
                    self.toast_message = "No unstaged files selected".into();
                    return Task::none();
                }
                let count = paths.len();
                for p in &paths {
                    self.selected_changed_files.remove(&(p.clone(), false));
                }
                self.run_git_op("Stage", move |svc, repo| async move {
                    svc.git
                        .stage(&repo.path, &paths)
                        .await
                        .map(|_| format!("Staged {count} file(s)"))
                })
            }
            Message::UnstageSelectedFiles => {
                if self.has_modal_or_menu_open()
                    || self.graph_selection != GraphSelection::WorkingTree
                {
                    return Task::none();
                }
                let paths = self.selected_staged_paths();
                if paths.is_empty() {
                    self.toast_is_error = false;
                    self.toast_message = "No staged files selected".into();
                    return Task::none();
                }
                let count = paths.len();
                for p in &paths {
                    self.selected_changed_files.remove(&(p.clone(), true));
                }
                self.run_git_op("Unstage", move |svc, repo| async move {
                    svc.git
                        .unstage(&repo.path, &paths)
                        .await
                        .map(|_| format!("Unstaged {count} file(s)"))
                })
            }
            Message::DiscardSelectedFiles => {
                if self.has_modal_or_menu_open()
                    || self.graph_selection != GraphSelection::WorkingTree
                    || self.focus_pane == ActiveFocusPane::TextInput
                {
                    return Task::none();
                }
                let paths = self.selected_discard_paths();
                if paths.is_empty() {
                    self.toast_is_error = false;
                    self.toast_message = "Select file(s) to discard".into();
                    return Task::none();
                }
                self.update(Message::OpenDiscardModal(Some(paths)))
            }
            Message::StageAll => self.run_git_op("Stage All", move |svc, repo| async move {
                svc.git.stage_all(&repo.path).await.map(|_| "Staged all".into())
            }),
            Message::UnstageAll => self.run_git_op("Unstage All", move |svc, repo| async move {
                svc.git
                    .unstage_all(&repo.path)
                    .await
                    .map(|_| "Unstaged all".into())
            }),
            Message::ApplyHunk {
                patch,
                cached,
                reverse,
            } => {
                let label = match (cached, reverse) {
                    (true, false) => "Stage Hunk",
                    (true, true) => "Unstage Hunk",
                    _ => "Discard Hunk",
                };
                self.run_git_op(label, move |svc, repo| async move {
                    svc.git
                        .apply_hunk(&repo.path, &patch, cached, reverse)
                        .await
                        .map(|_| "Applied hunk".into())
                })
            }
            Message::CommitMessageAction(action) => {
                self.terminal_focused = false;
                self.focus_pane = ActiveFocusPane::TextInput;
                self.commit_editor.perform(action);
                Task::none()
            }
            Message::AmendToggled(v) => {
                self.amend_commit = v;
                if v && let Some(repo) = self.current_repo() {
                    let svc = self.service.clone();
                    return Task::perform(
                        async move {
                            svc.git
                                .amend_target(&repo.path)
                                .await
                                .map_err(|e| e.to_string())
                        },
                        Message::AmendTargetLoaded,
                    );
                } else if !v {
                    self.amend_target = None;
                }
                Task::none()
            }
            Message::AmendTargetLoaded(res) => {
                if let Ok(target) = res {
                    if self.commit_editor.text().trim().is_empty() {
                        self.commit_editor = text_editor::Content::with_text(&target.message);
                    }
                    self.amend_target = Some(target);
                }
                Task::none()
            }
            Message::CommitSubmitted => self.try_commit(false),
            Message::CommitAndPushSubmitted => self.try_commit(true),
            Message::FetchSelected => self.run_git_op("Fetch", move |svc, repo| async move {
                svc.git.fetch(&repo.path).await
            }),
            Message::PullSelected => self.run_git_op("Pull", move |svc, repo| async move {
                svc.git.pull(&repo.path).await
            }),
            Message::PushSelected => self.run_git_op("Push", move |svc, repo| async move {
                svc.git.push(&repo.path).await
            }),
            Message::StashSelected => self.run_git_op("Stash", move |svc, repo| async move {
                svc.git.stash_push(&repo.path, None).await
            }),
            Message::GitActionDone(label, res) => {
                self.busy_label = None;
                match res {
                    Ok(_) => {
                        self.toast_is_error = false;
                        self.toast_message = format!("{label} succeeded");
                    }
                    Err(e) => {
                        self.toast_is_error = true;
                        self.toast_message = format!("{label} failed: {e}");
                    }
                }
                self.update(Message::RefreshSelected)
            }
            Message::SubmodulesLoaded(id, res) => {
                if let Ok(list) = res {
                    if !Self::is_submodule_repo_id(&id) {
                        self.repo_submodules.insert(id.clone(), list.clone());
                    }
                    if self.active_repo_key().as_ref() == Some(&id) {
                        self.submodules = list;
                    }
                }
                Task::none()
            }
            Message::WorktreesLoaded(id, res) => {
                if self.active_repo_key().as_ref() == Some(&id)
                    && let Ok(list) = res
                {
                    self.worktrees = list;
                }
                Task::none()
            }
            Message::SubmoduleUrlChanged(v) => {
                self.new_submodule_url = v;
                Task::none()
            }
            Message::SubmodulePathChanged(v) => {
                self.new_submodule_path = v;
                Task::none()
            }
            Message::AddSubmoduleSubmitted => {
                let url = self.new_submodule_url.trim().to_string();
                let dest = self.new_submodule_path.trim().to_string();
                if url.is_empty() || dest.is_empty() {
                    return Task::none();
                }
                self.new_submodule_url.clear();
                self.new_submodule_path.clear();
                self.run_git_op("Add Submodule", move |svc, repo| async move {
                    svc.git.submodule_add(&repo.path, &url, &dest).await
                })
            }
            Message::UpdateSubmodules => {
                self.run_git_op("Update Submodules", move |svc, repo| async move {
                    svc.git.submodule_update(&repo.path, None).await
                })
            }
            Message::SyncSubmodules => {
                self.run_git_op("Sync Submodules", move |svc, repo| async move {
                    svc.git.submodule_sync(&repo.path).await
                })
            }
            Message::RemoveSubmodule(sub_path) => {
                self.run_git_op("Remove Submodule", move |svc, repo| async move {
                    svc.git.submodule_remove(&repo.path, &sub_path).await
                })
            }
            Message::RunPushPreflight => {
                let Some(repo) = self.current_repo() else {
                    return Task::none();
                };
                let svc = self.service.clone();
                Task::perform(
                    async move {
                        svc.git
                            .push_preflight(&repo.path)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    Message::PushPreflightLoaded,
                )
            }
            Message::PushPreflightLoaded(res) => {
                match res {
                    Ok(orphans) => {
                        self.toast_is_error = !orphans.is_empty();
                        self.toast_message = if orphans.is_empty() {
                            "Push preflight OK — no orphan submodule commits".into()
                        } else {
                            format!(
                                "Preflight: {} submodule(s) have unpushed commits — use Ordered Push",
                                orphans.len()
                            )
                        };
                        self.orphan_gitlinks = orphans;
                    }
                    Err(e) => {
                        self.toast_is_error = true;
                        self.toast_message = format!("Preflight failed: {e}");
                    }
                }
                Task::none()
            }
            Message::RunOrderedPush => {
                let sub_paths: Vec<String> = if !self.orphan_gitlinks.is_empty() {
                    self.orphan_gitlinks
                        .iter()
                        .map(|o| o.submodule_path.clone())
                        .collect()
                } else {
                    self.submodules.iter().map(|s| s.path.clone()).collect()
                };
                self.orphan_gitlinks.clear();
                self.run_git_op("Ordered Push", move |svc, repo| async move {
                    svc.git.ordered_push(&repo.path, &sub_paths).await
                })
            }
            Message::NewWorktreeBranchChanged(v) => {
                self.new_worktree_branch = v;
                Task::none()
            }
            Message::AddWorktreeSubmitted => {
                let branch = self.new_worktree_branch.trim().to_string();
                if branch.is_empty() {
                    return Task::none();
                }
                let Some(repo) = self.current_repo() else {
                    return Task::none();
                };
                let safe_name = branch.replace('/', "-");
                let dest = repo
                    .path
                    .parent()
                    .unwrap_or(&repo.path)
                    .join(format!("{}-{safe_name}", repo.name));
                self.new_worktree_branch.clear();
                let exists_local = self
                    .branches
                    .iter()
                    .any(|b| !b.is_remote && b.name == branch);
                self.run_git_op("Add Worktree", move |svc, r| async move {
                    svc.git
                        .add_worktree(&r.path, &dest, &branch, !exists_local)
                        .await
                })
            }
            Message::RemoveWorktree(wt_path) => {
                self.run_git_op("Remove Worktree", move |svc, repo| async move {
                    svc.git.remove_worktree(&repo.path, &wt_path, false).await
                })
            }

                other => {
                    unhandled = Some(other);
                    Task::none()
                }
            }
        })();
        match unhandled {
            Some(msg) => Err(msg),
            None => Ok(task),
        }
    }
}
