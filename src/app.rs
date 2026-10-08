use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use arboard::Clipboard;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::db::Database;
use crate::github;
use crate::model::{GithubUpdate, Task};

const REFRESH_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Setup,
    Normal,
    CreateTask,
    EditDescription,
    Details,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateField {
    Title,
    Description,
}

type SyncResult = Result<Vec<GithubUpdate>>;

pub struct App {
    database: Database,
    pub repository: Option<String>,
    github_username: Option<String>,
    pub tasks: Vec<Task>,
    pub selected: usize,
    pub mode: Mode,
    pub input: String,
    pub description_input: String,
    pub create_field: CreateField,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub should_quit: bool,
    sync_receiver: Option<Receiver<SyncResult>>,
    last_sync_started: Option<Instant>,
}

impl App {
    pub fn new(database: Database) -> Result<Self> {
        let repository = database.repository()?;
        let github_username = database.github_username()?;
        let tasks = database.tasks()?;
        let mode = if repository.is_some() {
            Mode::Normal
        } else {
            Mode::Setup
        };
        let mut app = Self {
            database,
            repository,
            github_username,
            tasks,
            selected: 0,
            mode,
            input: String::new(),
            description_input: String::new(),
            create_field: CreateField::Title,
            error: None,
            notice: None,
            should_quit: false,
            sync_receiver: None,
            last_sync_started: None,
        };
        app.start_sync();
        Ok(app)
    }

    pub fn selected_task(&self) -> Option<&Task> {
        self.tasks.get(self.selected)
    }

    pub fn is_syncing(&self) -> bool {
        self.sync_receiver.is_some()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return Ok(());
        }

        match self.mode {
            Mode::Setup => self.handle_setup_key(key),
            Mode::Normal => self.handle_normal_key(key),
            Mode::CreateTask => self.handle_create_key(key),
            Mode::EditDescription => self.handle_edit_description_key(key),
            Mode::Details => self.handle_details_key(key),
        }
    }

    fn handle_setup_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Enter => {
                let repository = self.input.trim();
                if repository.is_empty() {
                    self.error = Some("Enter a repository as owner/name".into());
                    return Ok(());
                }

                let configuration =
                    github::validate_repository(repository).and_then(|repository| {
                        github::authenticated_username().map(|username| (repository, username))
                    });
                match configuration {
                    Ok((repository, username)) => {
                        self.database.set_repository(&repository)?;
                        self.database.set_github_username(&username)?;
                        self.repository = Some(repository.clone());
                        self.github_username = Some(username.clone());
                        self.input.clear();
                        self.error = None;
                        self.notice = Some(format!("Tracking {repository} as {username}"));
                        self.mode = Mode::Normal;
                        self.start_sync();
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
                self.error = None;
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.push(character);
                self.error = None;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('c') => {
                if self.ensure_github_username()? {
                    self.mode = Mode::CreateTask;
                    self.input.clear();
                    self.description_input.clear();
                    self.create_field = CreateField::Title;
                    self.error = None;
                }
            }
            KeyCode::Char('e') => self.start_editing_description(),
            KeyCode::Char('y') => self.copy_selected_branch(),
            KeyCode::Char('r') => {
                if self.is_syncing() {
                    self.notice = Some("A GitHub refresh is already running".into());
                } else {
                    self.start_sync();
                }
            }
            KeyCode::Enter if self.selected_task().is_some() => self.mode = Mode::Details,
            KeyCode::Up | KeyCode::Char('k') => self.select_previous(),
            KeyCode::Down | KeyCode::Char('j') => self.select_next(),
            KeyCode::Home => self.selected = 0,
            KeyCode::End if !self.tasks.is_empty() => self.selected = self.tasks.len() - 1,
            _ => {}
        }
        Ok(())
    }

    fn handle_create_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.input.clear();
                self.description_input.clear();
                self.error = None;
                self.mode = Mode::Normal;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.create_field = match self.create_field {
                    CreateField::Title => CreateField::Description,
                    CreateField::Description => CreateField::Title,
                };
            }
            KeyCode::Enter => {
                let title = self.input.trim().to_owned();
                if title.is_empty() {
                    self.error = Some("Task title cannot be empty".into());
                    return Ok(());
                }
                let username = self
                    .github_username
                    .as_deref()
                    .context("GitHub username is not configured")?;
                let description = self.description_input.trim().to_owned();
                let task = self.database.create_task(username, &title, &description)?;
                let id = task.id;
                let branch = task.branch_name.clone();
                self.reload_tasks(Some(id))?;
                self.input.clear();
                self.description_input.clear();
                self.error = None;
                self.notice = Some(format!("Created {branch}; press y to copy"));
                self.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                self.create_input().pop();
                self.error = None;
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.create_input().push(character);
                self.error = None;
            }
            _ => {}
        }
        Ok(())
    }

    fn create_input(&mut self) -> &mut String {
        match self.create_field {
            CreateField::Title => &mut self.input,
            CreateField::Description => &mut self.description_input,
        }
    }

    fn start_editing_description(&mut self) {
        let Some(description) = self.selected_task().map(|task| task.description.clone()) else {
            self.error = Some("There is no task to describe".into());
            return;
        };
        self.input = description;
        self.error = None;
        self.mode = Mode::EditDescription;
    }

    fn handle_edit_description_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.input.clear();
                self.error = None;
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => {
                let Some(id) = self.selected_task().map(|task| task.id) else {
                    self.mode = Mode::Normal;
                    return Ok(());
                };
                self.database.set_description(id, self.input.trim())?;
                self.reload_tasks(Some(id))?;
                self.input.clear();
                self.error = None;
                self.notice = Some("Description saved".into());
                self.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                self.input.pop();
                self.error = None;
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.push(character);
                self.error = None;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_details_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.mode = Mode::Normal,
            KeyCode::Char('e') => self.start_editing_description(),
            KeyCode::Char('y') => self.copy_selected_branch(),
            KeyCode::Char('q') => self.should_quit = true,
            _ => {}
        }
        Ok(())
    }

    fn copy_selected_branch(&mut self) {
        let Some(branch) = self.selected_task().map(|task| task.branch_name.clone()) else {
            self.error = Some("There is no task to copy".into());
            return;
        };

        let result = Clipboard::new()
            .context("clipboard is unavailable")
            .and_then(|mut clipboard| {
                clipboard
                    .set_text(branch.clone())
                    .context("could not write to the clipboard")
            });
        match result {
            Ok(()) => {
                self.error = None;
                self.notice = Some(format!("Copied {branch}"));
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn ensure_github_username(&mut self) -> Result<bool> {
        if self.github_username.is_some() {
            return Ok(true);
        }

        match github::authenticated_username() {
            Ok(username) => {
                self.database.set_github_username(&username)?;
                self.github_username = Some(username);
                Ok(true)
            }
            Err(error) => {
                self.error = Some(format!("Could not determine GitHub username: {error}"));
                self.notice = None;
                Ok(false)
            }
        }
    }

    fn select_previous(&mut self) {
        if !self.tasks.is_empty() {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    fn select_next(&mut self) {
        if self.selected + 1 < self.tasks.len() {
            self.selected += 1;
        }
    }

    fn reload_tasks(&mut self, preferred_id: Option<i64>) -> Result<()> {
        let current_id = preferred_id.or_else(|| self.selected_task().map(|task| task.id));
        self.tasks = self.database.tasks()?;
        self.selected = current_id
            .and_then(|id| self.tasks.iter().position(|task| task.id == id))
            .unwrap_or(0);
        Ok(())
    }

    pub fn start_sync(&mut self) {
        if self.sync_receiver.is_some() {
            return;
        }
        let Some(repository) = self.repository.clone() else {
            return;
        };
        let active_tasks = self
            .tasks
            .iter()
            .filter(|task| task.status == crate::model::TaskStatus::Active)
            .cloned()
            .collect::<Vec<_>>();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(github::synchronize(&repository, &active_tasks));
        });
        self.sync_receiver = Some(receiver);
        self.last_sync_started = Some(Instant::now());
        self.notice = Some("Refreshing GitHub status...".into());
    }

    pub fn poll_sync(&mut self) -> Result<()> {
        let result = match self.sync_receiver.as_ref().map(Receiver::try_recv) {
            Some(Ok(result)) => Some(result),
            Some(Err(TryRecvError::Disconnected)) => Some(Err(anyhow::anyhow!(
                "GitHub refresh worker stopped unexpectedly"
            ))),
            Some(Err(TryRecvError::Empty)) | None => None,
        };

        if let Some(result) = result {
            self.sync_receiver = None;
            match result {
                Ok(updates) => {
                    self.database.apply_github_updates(&updates)?;
                    self.reload_tasks(None)?;
                    self.error = None;
                    self.notice = Some("GitHub status refreshed".into());
                }
                Err(error) => {
                    self.error = Some(format!("GitHub refresh failed: {error}"));
                    self.notice = None;
                }
            }
        }
        Ok(())
    }

    pub fn start_periodic_sync_if_due(&mut self) {
        if self.sync_receiver.is_none()
            && self.repository.is_some()
            && self
                .last_sync_started
                .is_none_or(|started| started.elapsed() >= REFRESH_INTERVAL)
        {
            self.start_sync();
        }
    }
}
