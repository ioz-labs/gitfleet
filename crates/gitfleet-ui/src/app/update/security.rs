use crate::app::{GitFleetApp, Message};
use gitfleet_security::IgnoreLayer;
use iced::Task;
use iced::widget::text_editor;

impl GitFleetApp {
    pub(crate) fn try_update_security(&mut self, message: Message) -> Result<Task<Message>, Message> {
        let mut unhandled: Option<Message> = None;
        let task = (|| -> Task<Message> {
            match message {
            Message::HygieneScanned(id, secrets, oversized) => {
                if self.active_repo_key().as_ref() == Some(&id) {
                    self.staged_secrets = secrets;
                    self.oversized_files = oversized;
                }
                Task::none()
            }
            Message::AllowSecretLine(finding_id) => {
                self.service.security.allow_secret_line(finding_id.clone());
                self.staged_secrets.retain(|f| f.id != finding_id);
                Task::none()
            }
            Message::IgnoreOversizedFile(file_path) => {
                if let Some(repo) = self.current_repo() {
                    let _ = self.service.security.append_gitignore(&repo.path, &file_path);
                    let _ = self.update(Message::UnstageFile(file_path.clone()));
                    self.oversized_files.retain(|o| o.file_path != file_path);
                    self.toast_is_error = false;
                    self.toast_message = format!("Added '{file_path}' to .gitignore");
                }
                Task::none()
            }
            Message::CreateVaultKey => {
                match self.service.security.create_key() {
                    Ok(info) => {
                        self.toast_is_error = false;
                        self.toast_message = format!(
                            "Created Age key: {}",
                            info.public_key.unwrap_or_default()
                        );
                        self.refresh_vault_and_ignore();
                    }
                    Err(e) => {
                        self.toast_is_error = true;
                        self.toast_message = format!("Key create failed: {e}");
                    }
                }
                Task::none()
            }
            Message::CopyVaultPublicKey => {
                match self.service.security.copy_public_key() {
                    Ok(_) => {
                        self.toast_is_error = false;
                        self.toast_message = "Copied Age public key to clipboard".into();
                    }
                    Err(e) => {
                        self.toast_is_error = true;
                        self.toast_message = format!("Copy failed: {e}");
                    }
                }
                Task::none()
            }
            Message::CopyVaultPrivateKey => {
                match self.service.security.copy_private_key() {
                    Ok(_) => {
                        self.toast_is_error = false;
                        self.toast_message =
                            "Copied Age private key — paste into your password manager or another machine".into();
                    }
                    Err(e) => {
                        self.toast_is_error = true;
                        self.toast_message = format!("Copy private key failed: {e}");
                    }
                }
                Task::none()
            }
            Message::ToggleImportVaultKey => {
                self.show_import_key_bar = !self.show_import_key_bar;
                self.import_key_input.clear();
                Task::none()
            }
            Message::ImportVaultKeyInputChanged(v) => {
                self.import_key_input = v;
                Task::none()
            }
            Message::ImportVaultKeySubmitted => {
                let raw = self.import_key_input.clone();
                match self.service.security.import_key(&raw) {
                    Ok(info) => {
                        self.import_key_input.clear();
                        self.show_import_key_bar = false;
                        self.toast_is_error = false;
                        self.toast_message = format!(
                            "Imported Age key: {}",
                            info.public_key.unwrap_or_default()
                        );
                        self.refresh_vault_and_ignore();
                    }
                    Err(e) => {
                        self.toast_is_error = true;
                        self.toast_message = format!("Import key failed: {e}");
                    }
                }
                Task::none()
            }
            Message::ResetVaultKey => {
                let _ = self.service.security.reset_key();
                self.revealed_secrets.clear();
                self.refresh_vault_and_ignore();
                self.toast_is_error = false;
                self.toast_message = "Reset local Age identity key".into();
                Task::none()
            }
            Message::EncryptRepoEnv => {
                if let Some(repo) = self.current_repo() {
                    match self
                        .service
                        .security
                        .encrypt_selected_files(&repo.path, None)
                    {
                        Ok(n) => {
                            self.toast_is_error = false;
                            self.toast_message = format!(
                                "Encrypted {n} secret file(s) into .gitfleet/secrets.age (commit with .gitfleet/secrets.json)"
                            );
                            self.refresh_vault_and_ignore();
                            return self.load_status_task(repo);
                        }
                        Err(e) => {
                            self.toast_is_error = true;
                            self.toast_message = format!("Encrypt failed: {e}");
                        }
                    }
                }
                Task::none()
            }
            Message::RestoreRepoEnv => {
                if let Some(repo) = self.current_repo() {
                    match self
                        .service
                        .security
                        .restore_selected_files(&repo.path, None)
                    {
                        Ok(n) => {
                            self.toast_is_error = false;
                            self.toast_message =
                                format!("Restored {n} secret file(s) from encrypted vault");
                            self.refresh_vault_and_ignore();
                            return self.load_status_task(repo);
                        }
                        Err(e) => {
                            self.toast_is_error = true;
                            self.toast_message = format!("Restore failed: {e}");
                        }
                    }
                }
                Task::none()
            }
            Message::EncryptSingleSecretFile(path) => {
                if let Some(repo) = self.current_repo() {
                    match self
                        .service
                        .security
                        .encrypt_selected_files(&repo.path, Some(&[path.clone()]))
                    {
                        Ok(_) => {
                            self.toast_is_error = false;
                            self.toast_message =
                                format!("Encrypted '{path}' into .gitfleet/secrets.age");
                            self.refresh_vault_and_ignore();
                            return self.load_status_task(repo);
                        }
                        Err(e) => {
                            self.toast_is_error = true;
                            self.toast_message = format!("Encrypt failed: {e}");
                        }
                    }
                }
                Task::none()
            }
            Message::RestoreSingleSecretFile(path) => {
                if let Some(repo) = self.current_repo() {
                    match self
                        .service
                        .security
                        .restore_selected_files(&repo.path, Some(&[path.clone()]))
                    {
                        Ok(_) => {
                            self.toast_is_error = false;
                            self.toast_message = format!("Restored '{path}' from encrypted vault");
                            self.refresh_vault_and_ignore();
                            return self.load_status_task(repo);
                        }
                        Err(e) => {
                            self.toast_is_error = true;
                            self.toast_message = format!("Restore failed: {e}");
                        }
                    }
                }
                Task::none()
            }
            Message::ToggleSecretFilePicker => {
                self.show_secret_picker = !self.show_secret_picker;
                if self.show_secret_picker
                    && let Some(repo) = self.current_repo()
                {
                    self.secret_candidates = self
                        .service
                        .security
                        .discover_candidate_files(&repo.path);
                }
                Task::none()
            }
            Message::SecretAddPathChanged(v) => {
                self.secret_add_path_input = v;
                Task::none()
            }
            Message::AddSecretManifestFile(rel_path) => {
                let target = rel_path.trim().to_string();
                if target.is_empty() {
                    return Task::none();
                }
                if let Some(repo) = self.current_repo() {
                    match self
                        .service
                        .security
                        .add_manifest_file(&repo.path, &target, None)
                    {
                        Ok(_) => {
                            self.secret_add_path_input.clear();
                            self.secret_file_input = target.clone();
                            self.toast_is_error = false;
                            self.toast_message = format!(
                                "Added '{target}' to .gitfleet/secrets.json — click Encrypt to seal it"
                            );
                            self.refresh_vault_and_ignore();
                            return self.load_status_task(repo);
                        }
                        Err(e) => {
                            self.toast_is_error = true;
                            self.toast_message = format!("Cannot add secret file: {e}");
                        }
                    }
                }
                Task::none()
            }
            Message::PickSecretFile => {
                let start_dir = self.current_repo().map(|r| r.path);
                Task::perform(
                    async move {
                        let mut dlg = rfd::AsyncFileDialog::new()
                            .set_title("Choose secret file in repository (.env, .toml, .yaml)")
                            .add_filter(
                                "Secret files (.env, .toml, .yaml, .yml)",
                                &["env", "toml", "yaml", "yml"],
                            )
                            .add_filter("All files", &["*"]);
                        if let Some(dir) = start_dir {
                            dlg = dlg.set_directory(dir);
                        }
                        dlg.pick_file().await.map(|h| h.path().to_path_buf())
                    },
                    Message::SecretFilePicked,
                )
            }
            Message::SecretFilePicked(Some(full_path)) => {
                let path_str = full_path.to_string_lossy().into_owned();
                self.update(Message::AddSecretManifestFile(path_str))
            }
            Message::SecretFilePicked(None) => Task::none(),
            Message::RemoveSecretManifestFile(rel_path) => {
                if let Some(repo) = self.current_repo() {
                    match self
                        .service
                        .security
                        .remove_manifest_file(&repo.path, &rel_path, true)
                    {
                        Ok(_) => {
                            self.toast_is_error = false;
                            self.toast_message =
                                format!("Removed '{rel_path}' from .gitfleet/secrets.json");
                            self.refresh_vault_and_ignore();
                            return self.load_status_task(repo);
                        }
                        Err(e) => {
                            self.toast_is_error = true;
                            self.toast_message = format!("Remove failed: {e}");
                        }
                    }
                }
                Task::none()
            }
            Message::SelectSecretTargetFile(rel_path) => {
                self.secret_file_input = rel_path;
                Task::none()
            }
            Message::RevealSecret(file, key) => {
                if let Some(repo) = self.current_repo()
                    && let Ok(val) = self.service.security.reveal_secret(&repo.path, &file, &key)
                {
                    self.revealed_secrets.insert((file, key), val);
                }
                Task::none()
            }
            Message::CopySecret(file, key) => {
                if let Some(repo) = self.current_repo() {
                    match self
                        .service
                        .security
                        .copy_secret_value(&repo.path, &file, &key)
                    {
                        Ok(_) => {
                            self.toast_is_error = false;
                            self.toast_message = format!("Copied '{key}' directly to clipboard");
                        }
                        Err(e) => {
                            self.toast_is_error = true;
                            self.toast_message = format!("Copy failed: {e}");
                        }
                    }
                }
                Task::none()
            }
            Message::SecretFileInputChanged(v) => {
                self.secret_file_input = v;
                Task::none()
            }
            Message::SecretKeyInputChanged(v) => {
                self.secret_key_input = v;
                Task::none()
            }
            Message::SecretValInputChanged(v) => {
                self.secret_val_input = v;
                Task::none()
            }
            Message::SetSecretSubmitted => {
                let file = self.secret_file_input.trim().to_string();
                let key = self.secret_key_input.trim().to_string();
                let val = self.secret_val_input.clone();
                if file.is_empty() || key.is_empty() {
                    return Task::none();
                }
                if let Some(repo) = self.current_repo() {
                    match self
                        .service
                        .security
                        .set_secret_value(&repo.path, &file, &key, &val)
                    {
                        Ok(_) => {
                            self.secret_key_input.clear();
                            self.secret_val_input.clear();
                            self.toast_is_error = false;
                            self.toast_message = format!("Saved secret '{key}' into encrypted vault");
                            self.refresh_vault_and_ignore();
                        }
                        Err(e) => {
                            self.toast_is_error = true;
                            self.toast_message = format!("Save secret failed: {e}");
                        }
                    }
                }
                Task::none()
            }
            Message::SelectIgnoreLayer(layer) => {
                self.ignore_layer = layer;
                let text = match layer {
                    IgnoreLayer::RootGitignore => self.ignore_snapshot.root_gitignore.clone(),
                    IgnoreLayer::PersonalExclude => self.ignore_snapshot.personal_exclude.clone(),
                    IgnoreLayer::GlobalExclude => self.ignore_snapshot.global_exclude.clone(),
                };
                self.ignore_editor = text_editor::Content::with_text(&text);
                Task::none()
            }
            Message::IgnoreEditorAction(action) => {
                self.ignore_editor.perform(action);
                Task::none()
            }
            Message::SaveIgnoreLayer => {
                if let Some(repo) = self.current_repo() {
                    let content = self.ignore_editor.text();
                    match self
                        .service
                        .security
                        .write_ignore_layer(&repo.path, self.ignore_layer, &content)
                    {
                        Ok(_) => {
                            self.toast_is_error = false;
                            self.toast_message = format!("Saved {:?} ignore layer", self.ignore_layer);
                            self.refresh_vault_and_ignore();
                            return self.update(Message::RefreshSelected);
                        }
                        Err(e) => {
                            self.toast_is_error = true;
                            self.toast_message = format!("Save ignore failed: {e}");
                        }
                    }
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
