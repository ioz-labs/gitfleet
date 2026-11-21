use crate::app::{BottomLeftTab, BottomViewMode, GitFleetApp, GraphSelection, GraphTab, Message};
use chrono::Utc;
use gitfleet_core::UndoEntry;
use gitfleet_git::ResetMode;
use iced::Task;
use iced::widget::text_editor;
use std::time::{Duration, Instant};

impl GitFleetApp {
    pub(crate) fn try_update_git_history(&mut self, message: Message) -> Result<Task<Message>, Message> {
        let mut unhandled: Option<Message> = None;
        let task = (|| -> Task<Message> {
            match message {
            Message::SetBottomLeftTab(tab) => {
                self.bottom_left_tab = tab;
                if let Some(repo) = self.current_repo() {
                    return match tab {
                        BottomLeftTab::Branches => self.load_branches_task(repo),
                        BottomLeftTab::Stashes => self.load_stashes_task(repo),
                        BottomLeftTab::Tags => {
                            self.release_triggers =
                                self.service.git.read_release_triggers(&repo.path);
                            self.load_tags_task(repo)
                        }
                        BottomLeftTab::Submodules => self.load_submodules_task(repo),
                        BottomLeftTab::Worktrees => self.load_worktrees_task(repo),
                    };
                }
                Task::none()
            }
            Message::BranchesLoaded(id, res) => {
                if self.active_repo_key().as_ref() == Some(&id) {
                    self.branches = res.unwrap_or_default();
                }
                Task::none()
            }
            Message::StashesLoaded(id, res) => {
                if self.active_repo_key().as_ref() == Some(&id) {
                    self.stashes = res.unwrap_or_default();
                }
                Task::none()
            }
            Message::TagsLoaded(id, res) => {
                if self.active_repo_key().as_ref() == Some(&id) {
                    self.tags = res.unwrap_or_default();
                }
                Task::none()
            }
            Message::InspectStash(index) => {
                self.diff_is_working_file = false;
                if let Some(repo) = self.current_repo() {
                    let svc = self.service.clone();
                    let title = format!("stash@{{{index}}}");
                    return Task::perform(
                        async move {
                            let res = svc
                                .git
                                .stash_show(&repo.path, index)
                                .await
                                .map_err(|e| e.to_string());
                            (title, res)
                        },
                        |(t, r)| Message::DiffLoaded(t, r),
                    );
                }
                Task::none()
            }
            Message::ApplyStash(index) => {
                self.run_git_op("Apply Stash", move |svc, repo| async move {
                    svc.git.stash_apply(&repo.path, index).await
                })
            }
            Message::PopStash(index) => self.run_git_op("Pop Stash", move |svc, repo| async move {
                svc.git.stash_pop(&repo.path, index).await
            }),
            Message::DropStash(index) => {
                self.run_git_op("Drop Stash", move |svc, repo| async move {
                    svc.git.stash_drop(&repo.path, index).await
                })
            }
            Message::NewTagNameChanged(v) => {
                self.new_tag_name = v;
                Task::none()
            }
            Message::CreateTagSubmitted => {
                let name = self.new_tag_name.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                self.new_tag_name.clear();
                self.run_git_op("Create Tag", move |svc, repo| async move {
                    svc.git.create_tag(&repo.path, &name, None, None).await
                })
            }
            Message::CreateTagAtCommit(oid) => {
                let tag_name = format!("tag-{}", &oid[..oid.len().min(7)]);
                self.run_git_op("Create Tag", move |svc, repo| async move {
                    svc.git
                        .create_tag(&repo.path, &tag_name, Some(&oid), None)
                        .await
                })
            }
            Message::PushTag(name) => self.run_git_op("Push Tag", move |svc, repo| async move {
                svc.git.push_tags(&repo.path, "origin", &[name]).await
            }),
            Message::DeleteTag(name) => {
                self.run_git_op("Delete Tag", move |svc, repo| async move {
                    svc.git.delete_tag(&repo.path, &name, None).await
                })
            }
            Message::SetGraphTab(tab) => {
                self.graph_tab = tab;
                if tab == GraphTab::Reflog
                    && let Some(repo) = self.current_repo()
                {
                    return self.load_reflog_task(repo);
                }
                Task::none()
            }
            Message::ToggleGraphAllBranches => {
                if !self.graph_checked_branches.is_empty() {
                    self.graph_checked_branches.clear();
                    self.graph_all_branches = false;
                } else {
                    self.graph_all_branches = !self.graph_all_branches;
                }
                if let Some(repo) = self.current_repo() {
                    return self.load_log_task(repo);
                }
                Task::none()
            }
            Message::ToggleGraphBranch(name) => {
                if self.graph_checked_branches.is_empty() {
                    let head_ref = self
                        .branches
                        .iter()
                        .find(|b| !b.is_remote && b.is_head)
                        .map(|b| b.name.clone())
                        .unwrap_or_else(|| "HEAD".to_string());
                    if name != head_ref && name != "HEAD" {
                        self.graph_checked_branches.insert(head_ref);
                    }
                    self.graph_checked_branches.insert(name);
                } else if !self.graph_checked_branches.remove(&name) {
                    self.graph_checked_branches.insert(name);
                }
                if let Some(repo) = self.current_repo() {
                    return self.load_log_task(repo);
                }
                Task::none()
            }
            Message::ToggleGraphBranchGroup(names) => {
                let all_checked = !names.is_empty()
                    && names
                        .iter()
                        .all(|n| self.graph_checked_branches.contains(n));
                if all_checked {
                    for n in names {
                        self.graph_checked_branches.remove(&n);
                    }
                } else {
                    for n in names {
                        self.graph_checked_branches.insert(n);
                    }
                }
                if let Some(repo) = self.current_repo() {
                    return self.load_log_task(repo);
                }
                Task::none()
            }
            Message::ClearGraphCheckedBranches => {
                self.graph_checked_branches.clear();
                self.graph_all_branches = false;
                if let Some(repo) = self.current_repo() {
                    return self.load_log_task(repo);
                }
                Task::none()
            }
            Message::ToggleGraphOrder => {
                self.graph_topo_order = !self.graph_topo_order;
                if let Some(repo) = self.current_repo() {
                    return self.load_log_task(repo);
                }
                Task::none()
            }
            Message::ReflogLoaded(id, res) => {
                if self.active_repo_key().as_ref() == Some(&id) {
                    self.reflog = res.unwrap_or_default();
                }
                Task::none()
            }
            Message::ResetCommit(oid, mode) => {
                self.run_git_op("Reset", move |svc, repo| async move {
                    if let Ok(prev) = svc.git.head_oid(&repo.path).await {
                        svc.undo.record(UndoEntry {
                            id: format!("{}", Utc::now().timestamp_millis()),
                            repo_id: repo.id.clone(),
                            label: format!("Reset {mode:?} to {}", &oid[..oid.len().min(7)]),
                            previous_head: prev,
                            created_at: Utc::now(),
                        });
                    }
                    svc.git.reset_commit(&repo.path, &oid, mode).await
                })
            }
            Message::RevertCommit(oid) => {
                self.run_git_op("Revert", move |svc, repo| async move {
                    if let Ok(prev) = svc.git.head_oid(&repo.path).await {
                        svc.undo.record(UndoEntry {
                            id: format!("{}", Utc::now().timestamp_millis()),
                            repo_id: repo.id.clone(),
                            label: format!("Revert {}", &oid[..oid.len().min(7)]),
                            previous_head: prev,
                            created_at: Utc::now(),
                        });
                    }
                    svc.git.revert_commit(&repo.path, &oid).await
                })
            }
            Message::UndoLastAction => {
                let Some(repo) = self.current_repo() else {
                    return Task::none();
                };
                let Some(entry) = self.service.undo.pop(&repo.id) else {
                    self.toast_is_error = false;
                    self.toast_message = "Nothing to undo for this repository".into();
                    return Task::none();
                };
                let prev_head = entry.previous_head;
                self.run_git_op("Undo", move |svc, r| async move {
                    svc.git
                        .reset_commit(&r.path, &prev_head, ResetMode::Mixed)
                        .await
                })
            }
            Message::ToggleMergeModal => {
                if self.selected_repo.is_none() {
                    return Task::none();
                }
                self.show_palette = false;
                self.show_branch_popover = false;
                self.show_git_flow_modal = false;
                self.show_merge_modal = !self.show_merge_modal;
                self.merge_target_branch = None;
                self.merge_forecast = None;
                Task::none()
            }
            Message::SelectMergeTarget(branch) => {
                self.merge_target_branch = Some(branch.clone());
                self.merge_forecast = None;
                if let Some(repo) = self.current_repo() {
                    let svc = self.service.clone();
                    return Task::perform(
                        async move {
                            let res = svc
                                .git
                                .merge_forecast(&repo.path, &branch)
                                .await
                                .map_err(|e| e.to_string());
                            (branch, res)
                        },
                        |(b, r)| Message::MergeForecastLoaded(b, r),
                    );
                }
                Task::none()
            }
            Message::MergeForecastLoaded(branch, res) => {
                if self.merge_target_branch.as_deref() == Some(branch.as_str())
                    && let Ok(fc) = res
                {
                    self.merge_forecast = Some(fc);
                }
                Task::none()
            }
            Message::ExecuteMerge(branch) => {
                self.show_merge_modal = false;
                self.run_git_op("Merge", move |svc, repo| async move {
                    if let Ok(prev) = svc.git.head_oid(&repo.path).await {
                        svc.undo.record(UndoEntry {
                            id: format!("{}", Utc::now().timestamp_millis()),
                            repo_id: repo.id.clone(),
                            label: format!("Merge {branch}"),
                            previous_head: prev,
                            created_at: Utc::now(),
                        });
                    }
                    svc.git.merge_branch(&repo.path, &branch).await
                })
            }
            Message::ExecuteRebase(branch) => {
                self.show_merge_modal = false;
                self.run_git_op("Rebase", move |svc, repo| async move {
                    if let Ok(prev) = svc.git.head_oid(&repo.path).await {
                        svc.undo.record(UndoEntry {
                            id: format!("{}", Utc::now().timestamp_millis()),
                            repo_id: repo.id.clone(),
                            label: format!("Rebase onto {branch}"),
                            previous_head: prev,
                            created_at: Utc::now(),
                        });
                    }
                    svc.git.rebase_branch(&repo.path, &branch).await
                })
            }
            Message::ResolveConflict(path, strategy) => {
                self.run_git_op("Resolve Conflict", move |svc, repo| async move {
                    svc.git.resolve_conflict(&repo.path, &path, strategy).await
                })
            }
            Message::ContinueGitOp => {
                self.run_git_op("Continue", move |svc, repo| async move {
                    svc.git.continue_operation(&repo.path).await
                })
            }
            Message::AbortGitOp => self.run_git_op("Abort", move |svc, repo| async move {
                svc.git.abort_operation(&repo.path).await
            }),
            Message::ToggleGitFlowModal => {
                if self.selected_repo.is_none() {
                    return Task::none();
                }
                self.show_palette = false;
                self.show_branch_popover = false;
                self.show_merge_modal = false;
                self.show_git_flow_modal = !self.show_git_flow_modal;
                if self.show_git_flow_modal
                    && let Some(repo) = self.current_repo()
                {
                    let svc = self.service.clone();
                    return Task::perform(
                        async move {
                            svc.git
                                .get_git_flow_state(&repo.path)
                                .await
                                .map_err(|e| e.to_string())
                        },
                        Message::GitFlowStateLoaded,
                    );
                }
                Task::none()
            }
            Message::GitFlowStateLoaded(res) => {
                if let Ok(st) = res {
                    if let Some(m) = &st.main_branch {
                        self.git_flow_main_input = m.clone();
                    }
                    if let Some(d) = &st.develop_branch {
                        self.git_flow_develop_input = d.clone();
                    }
                    self.git_flow_state = st;
                }
                Task::none()
            }
            Message::GitFlowMainChanged(v) => {
                self.git_flow_main_input = v;
                Task::none()
            }
            Message::GitFlowDevelopChanged(v) => {
                self.git_flow_develop_input = v;
                Task::none()
            }
            Message::GitFlowNameChanged(v) => {
                self.git_flow_name_input = v;
                Task::none()
            }
            Message::GitFlowKindChanged(k) => {
                self.git_flow_kind = k;
                Task::none()
            }
            Message::InitGitFlow => {
                let m = self.git_flow_main_input.trim().to_string();
                let d = self.git_flow_develop_input.trim().to_string();
                if m.is_empty() || d.is_empty() {
                    return Task::none();
                }
                self.show_git_flow_modal = false;
                self.run_git_op("Init Git Flow", move |svc, repo| async move {
                    svc.git.init_git_flow(&repo.path, &m, &d).await
                })
            }
            Message::StartGitFlow => {
                let name = self.git_flow_name_input.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                let kind = self.git_flow_kind;
                self.git_flow_name_input.clear();
                self.show_git_flow_modal = false;
                self.run_git_op("Start Git Flow", move |svc, repo| async move {
                    svc.git.start_git_flow(&repo.path, kind, &name).await
                })
            }
            Message::FinishGitFlow => {
                let name = self.git_flow_name_input.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                let kind = self.git_flow_kind;
                self.git_flow_name_input.clear();
                self.show_git_flow_modal = false;
                self.run_git_op("Finish Git Flow", move |svc, repo| async move {
                    svc.git.finish_git_flow(&repo.path, kind, &name).await
                })
            }
            Message::BranchClicked(branch) => {
                const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(450);
                let now = Instant::now();
                let is_double = self.last_branch_click.as_ref().is_some_and(|(prev, t)| {
                    prev == &branch && now.duration_since(*t) <= DOUBLE_CLICK_WINDOW
                });
                self.selected_branch = Some(branch.clone());
                if is_double {
                    self.last_branch_click = None;
                    let is_already_head = self
                        .branches
                        .iter()
                        .any(|b| !b.is_remote && b.name == branch && b.is_head);
                    if is_already_head {
                        return Task::none();
                    }
                    let linked_wt_path = self
                        .worktrees
                        .iter()
                        .find(|w| !w.is_main && w.branch.as_deref() == Some(branch.as_str()))
                        .map(|w| w.path.display().to_string())
                        .or_else(|| {
                            self.branches
                                .iter()
                                .find(|b| !b.is_remote && !b.is_head && b.name == branch)
                                .and_then(|b| b.worktree_path.clone())
                        });
                    if let Some(wt_path) = linked_wt_path {
                        self.toast_is_error = false;
                        self.toast_message = format!(
                            "Branch '{branch}' is already checked out in worktree: {wt_path}"
                        );
                        return Task::none();
                    }
                    return self.update(Message::CheckoutBranch(branch));
                }
                self.last_branch_click = Some((branch, now));
                Task::none()
            }
            Message::CheckoutBranch(branch) => {
                self.show_branch_popover = false;
                self.selected_branch = Some(branch.clone());
                self.run_git_op("Checkout", move |svc, repo| async move {
                    svc.git.checkout(&repo.path, &branch).await
                })
            }
            Message::CheckoutRemoteBranch(remote_ref) => {
                self.show_branch_popover = false;
                self.run_git_op("Checkout Remote", move |svc, repo| async move {
                    svc.git.checkout_remote_branch(&repo.path, &remote_ref).await
                })
            }
            Message::CheckoutCommit(oid) => {
                self.run_git_op("Checkout Commit", move |svc, repo| async move {
                    svc.git.checkout_detached(&repo.path, &oid).await
                })
            }
            Message::FastForwardMergeBranch(branch) => {
                self.run_git_op("Fast-Forward Merge", move |svc, repo| async move {
                    if let Ok(prev) = svc.git.head_oid(&repo.path).await {
                        svc.undo.record(UndoEntry {
                            id: format!("{}", Utc::now().timestamp_millis()),
                            repo_id: repo.id.clone(),
                            label: format!("Fast-forward merge {branch}"),
                            previous_head: prev,
                            created_at: Utc::now(),
                        });
                    }
                    svc.git.merge_ff_only(&repo.path, &branch).await
                })
            }
            Message::PushBranch(branch) => {
                self.run_git_op("Push Branch", move |svc, repo| async move {
                    svc.git.push_branch(&repo.path, &branch).await
                })
            }
            Message::UnsetBranchUpstream(branch) => {
                self.run_git_op("Stop Tracking", move |svc, repo| async move {
                    svc.git.unset_upstream(&repo.path, &branch).await
                })
            }
            Message::CreateBranchSubmitted => {
                let name = self.branch_filter_query.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                self.branch_filter_query.clear();
                self.show_branch_popover = false;
                self.run_git_op("Create Branch", move |svc, repo| async move {
                    svc.git.create_branch(&repo.path, &name).await
                })
            }
            Message::DeleteBranch(name, force) => {
                self.graph_checked_branches.remove(&name);
                self.run_git_op("Delete Branch", move |svc, repo| async move {
                    svc.git.delete_branch(&repo.path, &name, force).await
                })
            }
            Message::DeleteBranches(names, force) => {
                for n in &names {
                    self.graph_checked_branches.remove(n);
                }
                self.run_git_op("Delete Branches", move |svc, repo| async move {
                    svc.git.delete_branches(&repo.path, &names, force).await
                })
            }
            Message::CompareRefs(base_ref, target_ref) => {
                self.diff_is_working_file = false;
                if matches!(self.bottom_mode, BottomViewMode::Preview | BottomViewMode::Edit) {
                    self.bottom_mode = BottomViewMode::SideBySideDiff;
                }
                if let Some(repo) = self.current_repo() {
                    let svc = self.service.clone();
                    let title = format!("Compare {base_ref} ↔ {target_ref}");
                    return Task::perform(
                        async move {
                            let res = svc
                                .git
                                .diff_refs(&repo.path, &base_ref, &target_ref)
                                .await
                                .map_err(|e| e.to_string());
                            (title, res)
                        },
                        |(t, r)| Message::DiffLoaded(t, r),
                    );
                }
                Task::none()
            }
            Message::RevealBranchCommit(branch) => {
                if let Some(repo) = self.current_repo() {
                    let mut tasks = Vec::new();
                    if !self.graph_checked_branches.is_empty()
                        && self.graph_checked_branches.insert(branch.clone())
                    {
                        tasks.push(self.load_log_task(repo.clone()));
                    }
                    let svc = self.service.clone();
                    tasks.push(Task::perform(
                        async move {
                            svc.git
                                .rev_parse(&repo.path, &branch)
                                .await
                                .map_err(|e| e.to_string())
                        },
                        Message::BranchCommitResolved,
                    ));
                    return Task::batch(tasks);
                }
                Task::none()
            }
            Message::BranchCommitResolved(res) => match res {
                Ok(oid) => self.update(Message::SelectCommit(oid)),
                Err(e) => {
                    self.toast_is_error = true;
                    self.toast_message = format!("Could not resolve branch commit: {e}");
                    Task::none()
                }
            },
            Message::UseCommitMessage(oid) => {
                if let Some(c) = self.commits.iter().find(|c| c.oid == oid) {
                    let full = if c.body.trim().is_empty() {
                        c.summary.clone()
                    } else {
                        format!("{}\n\n{}", c.summary, c.body)
                    };
                    self.commit_editor = text_editor::Content::with_text(&full);
                    self.graph_selection = GraphSelection::WorkingTree;
                    self.toast_is_error = false;
                    self.toast_message =
                        format!("Loaded message from {} into Commit box", c.short_oid);
                }
                Task::none()
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
