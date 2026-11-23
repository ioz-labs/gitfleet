//! `F0` — Async handlers for Portable Group / Workspace Blueprint Export & Batch Auto-Clone Import.

use crate::app::{GitFleetApp, Message};
use crate::views::overlays::blueprint_modal::{
    BlueprintExportModalState, BlueprintExportRepoItem, BlueprintImportItem,
    BlueprintImportModalState, BlueprintItemStatus,
};
use chrono::Utc;
use gitfleet_git::{CloneOptions, CloneOutcome, CloneProgressTracker};
use gitfleet_workspace::{
    BLUEPRINT_SCHEMA_VERSION, BlueprintRemote, BlueprintRepo, GroupBlueprint, UrlProtocolMode,
    compute_export_group_path, group_contains_repo, normalize_group_path,
    resolve_materialized_repo_dest, rewrite_git_url_protocol,
};
use iced::Task;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Semaphore;

impl GitFleetApp {
    pub(crate) fn try_update_blueprint(&mut self, message: Message) -> Result<Task<Message>, Message> {
        match message {
            Message::OpenExportBlueprintModal(group_opt) => {
                self.show_palette = false;
                self.show_workspace_modal = false;
                self.context_menu = None;

                let ws = self.service.workspace.current();
                let norm_sel = group_opt
                    .as_deref()
                    .map(normalize_group_path)
                    .filter(|g| !g.is_empty());

                let (title, target_repos, target_groups) = if let Some(ref grp) = norm_sel {
                    let repos: Vec<_> = ws
                        .repos
                        .iter()
                        .filter(|r| {
                            !Self::is_submodule_repo_id(&r.id)
                                && group_contains_repo(grp, r.group.as_deref())
                        })
                        .cloned()
                        .collect();
                    let groups: Vec<_> = ws
                        .groups
                        .iter()
                        .filter(|g| group_contains_repo(grp, Some(g.as_str())))
                        .cloned()
                        .collect();
                    (format!("Export Group Blueprint: {grp}"), repos, groups)
                } else {
                    let repos: Vec<_> = ws
                        .repos
                        .iter()
                        .filter(|r| !Self::is_submodule_repo_id(&r.id))
                        .cloned()
                        .collect();
                    (
                        format!("Export Workspace Blueprint: {}", ws.name),
                        repos,
                        ws.groups.clone(),
                    )
                };

                self.blueprint_export_modal = Some(BlueprintExportModalState {
                    title,
                    selected_group: norm_sel,
                    relative_to_selected_group: false,
                    loading: true,
                    groups: target_groups,
                    items: Vec::new(),
                });

                let svc = self.service.clone();
                let statuses = self.statuses.clone();
                Ok(Task::perform(
                    async move {
                        let mut out = Vec::with_capacity(target_repos.len());
                        for repo in target_repos {
                            let remotes = svc.git.get_remotes(&repo.path).await.unwrap_or_default();
                            let origin = remotes
                                .iter()
                                .find(|r| r.name == "origin")
                                .or_else(|| remotes.first())
                                .cloned();
                            let extra_remotes: Vec<BlueprintRemote> = remotes
                                .into_iter()
                                .filter(|r| {
                                    origin.as_ref().is_none_or(|o| o.name != r.name)
                                })
                                .map(|r| BlueprintRemote {
                                    name: r.name,
                                    fetch_url: r.fetch_url,
                                    push_url: r.push_url,
                                })
                                .collect();

                            let branch = match statuses.get(&repo.id).and_then(|s| s.branch.clone())
                            {
                                Some(b) if b != "HEAD" && !b.starts_with('(') => Some(b),
                                _ => svc
                                    .git
                                    .get_status(&repo.path)
                                    .await
                                    .ok()
                                    .and_then(|s| s.branch)
                                    .filter(|b| b != "HEAD" && !b.starts_with('(')),
                            };

                            let folder_name = repo
                                .path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(&repo.name)
                                .to_string();
                            let has_submodules = repo.path.join(".gitmodules").is_file();
                            let gf_dir = repo.path.join(".gitfleet");
                            let has_secrets_vault = gf_dir.join("secrets.age").is_file()
                                || gf_dir.join("secrets.json").is_file();
                            let has_origin = origin.is_some();

                            out.push(BlueprintExportRepoItem {
                                repo_id: repo.id,
                                name: repo.name,
                                folder_name,
                                raw_group: repo.group,
                                origin_url: origin.map(|o| o.fetch_url),
                                extra_remotes,
                                branch,
                                has_submodules,
                                has_secrets_vault,
                                pinned: repo.pinned,
                                selected: has_origin,
                            });
                        }
                        out
                    },
                    Message::BlueprintExportLoaded,
                ))
            }

            Message::BlueprintExportLoaded(items) => {
                if let Some(modal) = &mut self.blueprint_export_modal {
                    modal.items = items;
                    modal.loading = false;
                }
                Ok(Task::none())
            }

            Message::SetBlueprintExportRelative(rel) => {
                if let Some(modal) = &mut self.blueprint_export_modal {
                    modal.relative_to_selected_group = rel;
                }
                Ok(Task::none())
            }

            Message::ToggleBlueprintExportRepo(idx, checked) => {
                if let Some(modal) = &mut self.blueprint_export_modal
                    && let Some(item) = modal.items.get_mut(idx)
                    && item.origin_url.is_some()
                {
                    item.selected = checked;
                }
                Ok(Task::none())
            }

            Message::PickSaveBlueprintFile => {
                let Some(modal) = &self.blueprint_export_modal else {
                    return Ok(Task::none());
                };
                let slug_src = modal
                    .selected_group
                    .as_deref()
                    .map(|g| g.replace('/', "-"))
                    .unwrap_or_else(|| self.service.workspace.current().name.replace(' ', "-"));
                let clean_slug: String = slug_src
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '-' || c == '_' {
                            c.to_ascii_lowercase()
                        } else {
                            '-'
                        }
                    })
                    .collect();
                let default_file = format!("{clean_slug}.gitfleet-pack.json");

                Ok(Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_title("Export GitFleet Blueprint (.gitfleet-pack.json)")
                            .set_file_name(default_file)
                            .add_filter("GitFleet Blueprint", &["json"])
                            .save_file()
                            .await
                            .map(|f| f.path().to_path_buf())
                    },
                    Message::SaveBlueprintFilePicked,
                ))
            }

            Message::SaveBlueprintFilePicked(Some(path)) => {
                let Some(modal) = self.blueprint_export_modal.take() else {
                    return Ok(Task::none());
                };

                let mut exported_groups = Vec::new();
                for g in &modal.groups {
                    if let Some(mapped) = compute_export_group_path(
                        Some(g.as_str()),
                        modal.selected_group.as_deref(),
                        modal.relative_to_selected_group,
                    ) && !exported_groups.contains(&mapped)
                    {
                        exported_groups.push(mapped);
                    }
                }

                let mut repos = Vec::new();
                for item in modal.items {
                    if !item.selected {
                        continue;
                    }
                    let Some(origin_url) = item.origin_url else {
                        continue;
                    };
                    let group_path = compute_export_group_path(
                        item.raw_group.as_deref(),
                        modal.selected_group.as_deref(),
                        modal.relative_to_selected_group,
                    );
                    repos.push(BlueprintRepo {
                        name: item.name,
                        folder_name: item.folder_name,
                        group_path,
                        origin_url,
                        extra_remotes: item.extra_remotes,
                        branch: item.branch,
                        has_submodules: item.has_submodules,
                        has_secrets_vault: item.has_secrets_vault,
                        pinned: item.pinned,
                    });
                }

                let repo_count = repos.len();
                let bp = GroupBlueprint {
                    version: BLUEPRINT_SCHEMA_VERSION,
                    title: modal.title,
                    source_group: modal.selected_group,
                    exported_at_utc: Utc::now().to_rfc3339(),
                    groups: exported_groups,
                    repos,
                };

                match bp.to_pretty_json() {
                    Ok(json) => match std::fs::write(&path, json) {
                        Ok(()) => {
                            self.toast_is_error = false;
                            self.toast_message = format!(
                                "Exported {repo_count} repo(s) blueprint to {}",
                                path.display()
                            );
                        }
                        Err(e) => {
                            self.report_action_error(
                                "Blueprint Export Failed",
                                format!("Could not write {}: {e}", path.display()),
                                false,
                            );
                        }
                    },
                    Err(err) => {
                        self.report_action_error("Blueprint Export Failed", err, false);
                    }
                }
                Ok(Task::none())
            }

            Message::SaveBlueprintFilePicked(None) => Ok(Task::none()),

            Message::PickImportBlueprintFile(parent_prefix) => {
                self.show_palette = false;
                self.show_workspace_modal = false;
                self.context_menu = None;

                Ok(Task::perform(
                    async move {
                        let picked = rfd::AsyncFileDialog::new()
                            .set_title("Import GitFleet Blueprint (.gitfleet-pack.json)")
                            .add_filter("GitFleet Blueprint", &["json"])
                            .pick_file()
                            .await
                            .map(|f| f.path().to_path_buf());
                        (parent_prefix, picked)
                    },
                    |(parent_prefix, path)| Message::ImportBlueprintFilePicked {
                        parent_prefix,
                        path,
                    },
                ))
            }

            Message::ImportBlueprintFilePicked {
                parent_prefix,
                path: Some(path),
            } => {
                let raw = match std::fs::read_to_string(&path) {
                    Ok(s) => s,
                    Err(e) => {
                        self.report_action_error(
                            "Blueprint Import Failed",
                            format!("Failed to read {}: {e}", path.display()),
                            false,
                        );
                        return Ok(Task::none());
                    }
                };
                let bp = match GroupBlueprint::from_json_str(&raw) {
                    Ok(b) => b,
                    Err(err) => {
                        self.report_action_error("Invalid Blueprint File", err, false);
                        return Ok(Task::none());
                    }
                };

                let default_target = self
                    .suggested_repo_parent_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                let recurse_submodules = bp.repos.iter().any(|r| r.has_submodules);
                let items: Vec<BlueprintImportItem> = bp
                    .repos
                    .iter()
                    .cloned()
                    .map(|repo| BlueprintImportItem {
                        repo,
                        selected: true,
                        status: BlueprintItemStatus::ReadyToClone,
                        tracker: None,
                        snapshot: None,
                    })
                    .collect();

                let mut state = BlueprintImportModalState {
                    source_file: path,
                    blueprint: bp,
                    target_base_dir: default_target,
                    parent_group_prefix: parent_prefix.unwrap_or_default(),
                    protocol_mode: UrlProtocolMode::KeepOriginal,
                    recurse_submodules,
                    items,
                    running: false,
                    finished_once: false,
                    cancel_flag: None,
                };
                state.refresh_preflight();
                self.blueprint_import_modal = Some(state);
                Ok(Task::none())
            }

            Message::ImportBlueprintFilePicked { path: None, .. } => Ok(Task::none()),

            Message::BlueprintTargetDirChanged(dir) => {
                if let Some(modal) = &mut self.blueprint_import_modal {
                    modal.target_base_dir = dir;
                    modal.refresh_preflight();
                }
                Ok(Task::none())
            }

            Message::PickBlueprintTargetDir => {
                let start_dir = self
                    .blueprint_import_modal
                    .as_ref()
                    .map(|m| PathBuf::from(m.target_base_dir.trim()))
                    .filter(|p| p.is_dir())
                    .or_else(|| self.suggested_repo_parent_dir());

                Ok(Task::perform(
                    async move {
                        let mut dlg = rfd::AsyncFileDialog::new()
                            .set_title("Select Target Base Directory for Blueprint Auto-Clone");
                        if let Some(dir) = start_dir {
                            dlg = dlg.set_directory(dir);
                        }
                        dlg.pick_folder().await.map(|f| f.path().to_path_buf())
                    },
                    Message::BlueprintTargetDirPicked,
                ))
            }

            Message::BlueprintTargetDirPicked(Some(dir)) => {
                self.last_repo_parent_dir = Some(dir.clone());
                if let Some(modal) = &mut self.blueprint_import_modal {
                    modal.target_base_dir = dir.display().to_string();
                    modal.refresh_preflight();
                }
                Ok(Task::none())
            }

            Message::BlueprintTargetDirPicked(None) => Ok(Task::none()),

            Message::BlueprintParentPrefixChanged(prefix) => {
                if let Some(modal) = &mut self.blueprint_import_modal {
                    modal.parent_group_prefix = prefix;
                    modal.refresh_preflight();
                }
                Ok(Task::none())
            }

            Message::SetBlueprintProtocolMode(mode) => {
                if let Some(modal) = &mut self.blueprint_import_modal
                    && !modal.running
                {
                    modal.protocol_mode = mode;
                }
                Ok(Task::none())
            }

            Message::ToggleBlueprintSubmodules(v) => {
                if let Some(modal) = &mut self.blueprint_import_modal
                    && !modal.running
                {
                    modal.recurse_submodules = v;
                }
                Ok(Task::none())
            }

            Message::ToggleBlueprintImportRepo(idx, v) => {
                if let Some(modal) = &mut self.blueprint_import_modal
                    && !modal.running
                    && let Some(item) = modal.items.get_mut(idx)
                {
                    item.selected = v;
                }
                Ok(Task::none())
            }

            Message::StartBlueprintImport => Ok(self.launch_blueprint_import_batch(false)),

            Message::RetryFailedBlueprintImport => Ok(self.launch_blueprint_import_batch(true)),

            Message::BlueprintImportTick => {
                if let Some(modal) = &mut self.blueprint_import_modal
                    && modal.running
                {
                    for item in &mut modal.items {
                        if item.status == BlueprintItemStatus::Cloning
                            && let Some(tracker) = &item.tracker
                        {
                            item.snapshot = Some(tracker.snapshot());
                        }
                    }
                }
                Ok(Task::none())
            }

            Message::CancelBlueprintImport => {
                if let Some(modal) = &mut self.blueprint_import_modal {
                    if let Some(flag) = &modal.cancel_flag {
                        flag.store(true, Ordering::SeqCst);
                    }
                    for item in &modal.items {
                        if let Some(tracker) = &item.tracker {
                            tracker.cancel();
                        }
                    }
                }
                Ok(Task::none())
            }

            Message::BlueprintRepoFinished { index, result } => {
                let mut follow_up_tasks = Vec::new();
                let mut batch_done = false;
                let mut added_entry = None;

                if let Some(modal) = &mut self.blueprint_import_modal {
                    if let Some(item) = modal.items.get_mut(index) {
                        item.tracker = None;
                        match result {
                            Ok((repo_dir, final_group, linked_existing)) => {
                                item.status = BlueprintItemStatus::Completed { linked_existing };
                                if let Ok(entry) = self
                                    .service
                                    .workspace
                                    .add_repo(repo_dir, final_group.clone())
                                {
                                    let _ = self
                                        .service
                                        .workspace
                                        .set_repo_group(&entry.id, final_group);
                                    if item.repo.pinned && !entry.pinned {
                                        let _ = self.service.workspace.toggle_pin(&entry.id);
                                    }
                                    added_entry = Some(entry);
                                }
                            }
                            Err(err) => {
                                if err == "__CANCELLED__" {
                                    item.status = BlueprintItemStatus::Skipped;
                                } else {
                                    item.status = BlueprintItemStatus::Failed(err);
                                }
                            }
                        }
                    }

                    let still_running = modal
                        .items
                        .iter()
                        .any(|i| i.status == BlueprintItemStatus::Cloning);
                    if !still_running {
                        modal.running = false;
                        modal.finished_once = true;
                        batch_done = true;

                        // Ensure all blueprint groups (including empty subgroups) exist in Workspace & on disk
                        let prefix_norm = normalize_group_path(&modal.parent_group_prefix);
                        let base_dir = PathBuf::from(modal.target_base_dir.trim());
                        for grp in &modal.blueprint.groups {
                            let full_grp = if prefix_norm.is_empty() {
                                grp.clone()
                            } else {
                                format!("{prefix_norm}/{grp}")
                            };
                            let _ = self.service.workspace.add_group(full_grp.clone());
                            let mut disk_grp = base_dir.clone();
                            for seg in full_grp.split('/') {
                                disk_grp.push(seg);
                            }
                            let _ = std::fs::create_dir_all(disk_grp);
                        }
                    }
                }

                let newly_added_id = added_entry.as_ref().map(|e| e.id.clone());
                if let Some(entry) = added_entry {
                    follow_up_tasks.push(self.load_status_task(entry.clone()));
                    follow_up_tasks.push(self.load_submodules_task(entry));
                }

                if batch_done {
                    if let Some(modal) = &self.blueprint_import_modal {
                        let ok_cnt = modal
                            .items
                            .iter()
                            .filter(|i| matches!(i.status, BlueprintItemStatus::Completed { .. }))
                            .count();
                        let fail_cnt = modal
                            .items
                            .iter()
                            .filter(|i| matches!(i.status, BlueprintItemStatus::Failed(_)))
                            .count();
                        self.toast_is_error = fail_cnt > 0;
                        self.toast_message = if fail_cnt > 0 {
                            format!(
                                "Blueprint finished: {ok_cnt} materialized, {fail_cnt} failed (use Retry Failed)"
                            )
                        } else {
                            format!(
                                "Blueprint complete: {ok_cnt} repository(s) materialized with Group = Folder"
                            )
                        };
                    }
                    if self.selected_repo.is_none()
                        && let Some(first_id) = newly_added_id
                    {
                        follow_up_tasks.push(self.update(Message::SelectRepo(first_id)));
                    }
                }

                Ok(Task::batch(follow_up_tasks))
            }

            other => Err(other),
        }
    }

    fn launch_blueprint_import_batch(&mut self, retry_only_failed: bool) -> Task<Message> {
        let Some(modal) = &mut self.blueprint_import_modal else {
            return Task::none();
        };
        if modal.running {
            return Task::none();
        }

        let base_str = modal.target_base_dir.trim().to_string();
        if base_str.is_empty() {
            self.report_action_error(
                "Target Directory Required",
                "Choose a Target Base Directory where the group folders and repositories will be created.",
                false,
            );
            return Task::none();
        }

        let base_path = PathBuf::from(&base_str);
        self.last_repo_parent_dir = Some(base_path.clone());
        let prefix_opt = {
            let t = modal.parent_group_prefix.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        };

        if !retry_only_failed {
            modal.refresh_preflight();
        }

        let sem = Arc::new(Semaphore::new(4));
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let protocol_mode = modal.protocol_mode;
        let recurse_submodules = modal.recurse_submodules;
        let mut tasks = Vec::new();

        for (idx, item) in modal.items.iter_mut().enumerate() {
            let eligible = if retry_only_failed {
                item.selected && matches!(item.status, BlueprintItemStatus::Failed(_))
            } else {
                item.selected
                    && matches!(
                        item.status,
                        BlueprintItemStatus::ReadyToClone | BlueprintItemStatus::ExistingGitLink
                    )
            };
            if !eligible {
                continue;
            }

            let resolved = match resolve_materialized_repo_dest(
                &base_path,
                item.repo.group_path.as_deref(),
                prefix_opt.as_deref(),
                &item.repo.folder_name,
            ) {
                Ok(r) => r,
                Err(err) => {
                    item.status = BlueprintItemStatus::Failed(err);
                    continue;
                }
            };

            let tracker = CloneProgressTracker::new();
            item.tracker = Some(tracker.clone());
            item.snapshot = Some(tracker.snapshot());
            item.status = BlueprintItemStatus::Cloning;

            let svc = self.service.clone();
            let sem_clone = sem.clone();
            let cancel_clone = cancel_flag.clone();
            let bp_repo = item.repo.clone();
            let (parent_dir, repo_dir, final_group) = resolved;

            tasks.push(Task::perform(
                async move {
                    let _permit = sem_clone.acquire_owned().await;
                    if cancel_clone.load(Ordering::SeqCst) || tracker.is_cancelled() {
                        return (idx, Err("__CANCELLED__".to_string()));
                    }

                    if let Err(e) = std::fs::create_dir_all(&parent_dir) {
                        return (
                            idx,
                            Err(format!(
                                "Failed to create folder '{}': {e}",
                                parent_dir.display()
                            )),
                        );
                    }

                    let origin_url = rewrite_git_url_protocol(&bp_repo.origin_url, protocol_mode);
                    let extra_remotes = bp_repo.extra_remotes.clone();

                    // Resume-safe path: if .git already exists on disk, wire remotes & fetch!
                    if repo_dir.join(".git").exists() {
                        let _ = svc
                            .git
                            .ensure_remote(&repo_dir, "origin", &origin_url, None)
                            .await;
                        for rem in &extra_remotes {
                            let furl = rewrite_git_url_protocol(&rem.fetch_url, protocol_mode);
                            let purl = rem
                                .push_url
                                .as_deref()
                                .map(|u| rewrite_git_url_protocol(u, protocol_mode));
                            let _ = svc
                                .git
                                .ensure_remote(&repo_dir, &rem.name, &furl, purl.as_deref())
                                .await;
                        }
                        let _ = svc.git.fetch(&repo_dir).await;
                        return (idx, Ok((repo_dir, final_group, true)));
                    }

                    let opts = CloneOptions {
                        url: origin_url,
                        parent_dir,
                        folder_name: bp_repo.folder_name.clone(),
                        branch: bp_repo.branch.clone(),
                        depth_one: false,
                        recurse_submodules: recurse_submodules || bp_repo.has_submodules,
                    };

                    match svc.git.clone_repo_with_progress(opts, tracker).await {
                        Ok(CloneOutcome::Completed(cloned_path)) => {
                            for rem in &extra_remotes {
                                let furl = rewrite_git_url_protocol(&rem.fetch_url, protocol_mode);
                                let purl = rem
                                    .push_url
                                    .as_deref()
                                    .map(|u| rewrite_git_url_protocol(u, protocol_mode));
                                let _ = svc
                                    .git
                                    .ensure_remote(
                                        &cloned_path,
                                        &rem.name,
                                        &furl,
                                        purl.as_deref(),
                                    )
                                    .await;
                            }
                            (idx, Ok((cloned_path, final_group, false)))
                        }
                        Ok(CloneOutcome::Cancelled) => (idx, Err("__CANCELLED__".to_string())),
                        Err(err) => {
                            // Fallback: if specified branch wasn't on remote anymore, retry default branch once
                            if opts_had_missing_branch(&err.to_string(), bp_repo.branch.as_deref())
                                && let Some(parent) = Path::new(&repo_dir).parent()
                            {
                                let retry_tracker = CloneProgressTracker::new();
                                let fallback_opts = CloneOptions {
                                    url: rewrite_git_url_protocol(
                                        &bp_repo.origin_url,
                                        protocol_mode,
                                    ),
                                    parent_dir: parent.to_path_buf(),
                                    folder_name: bp_repo.folder_name.clone(),
                                    branch: None,
                                    depth_one: false,
                                    recurse_submodules: recurse_submodules || bp_repo.has_submodules,
                                };
                                if let Ok(CloneOutcome::Completed(cloned_path)) = svc
                                    .git
                                    .clone_repo_with_progress(fallback_opts, retry_tracker)
                                    .await
                                {
                                    return (idx, Ok((cloned_path, final_group, false)));
                                }
                            }
                            (idx, Err(err.to_string()))
                        }
                    }
                },
                |(index, result)| Message::BlueprintRepoFinished { index, result },
            ));
        }

        if tasks.is_empty() {
            return Task::none();
        }

        modal.running = true;
        modal.cancel_flag = Some(cancel_flag);
        Task::batch(tasks)
    }
}

fn opts_had_missing_branch(err_msg: &str, branch: Option<&str>) -> bool {
    branch.is_some()
        && (err_msg.contains("Remote branch")
            || err_msg.contains("not found in upstream origin")
            || err_msg.contains("did not match any file(s)"))
}
