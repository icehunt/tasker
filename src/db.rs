use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};

use crate::model::{GithubStatus, GithubUpdate, Task, TaskStatus};

pub struct Database {
    connection: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }

        let connection =
            Connection::open(path).with_context(|| format!("could not open {}", path.display()))?;
        let mut database = Self { connection };
        database.migrate()?;
        Ok(database)
    }

    fn migrate(&mut self) -> Result<()> {
        self.connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS settings (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS tasks (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 title TEXT NOT NULL,
                 branch_name TEXT NOT NULL UNIQUE,
                 status TEXT NOT NULL DEFAULT 'active',
                 github_status TEXT NOT NULL DEFAULT 'not_pushed',
                 pr_number INTEGER,
                 pr_url TEXT,
                 created_at INTEGER NOT NULL,
                 completed_at INTEGER,
                 last_checked_at INTEGER
             );",
        )?;

        let version: i64 = self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < 2 {
            let transaction = self.connection.transaction()?;
            transaction.execute_batch(
                "ALTER TABLE tasks ADD COLUMN description TEXT NOT NULL DEFAULT '';
                 PRAGMA user_version = 2;",
            )?;
            transaction.commit()?;
        }
        Ok(())
    }

    pub fn repository(&self) -> Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT value FROM settings WHERE key = 'repository'",
                [],
                |row| row.get(0),
            )
            .optional()
            .context("could not load repository setting")
    }

    pub fn set_repository(&self, repository: &str) -> Result<()> {
        self.connection.execute(
            "INSERT INTO settings (key, value) VALUES ('repository', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [repository],
        )?;
        Ok(())
    }

    pub fn github_username(&self) -> Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT value FROM settings WHERE key = 'github_username'",
                [],
                |row| row.get(0),
            )
            .optional()
            .context("could not load GitHub username setting")
    }

    pub fn set_github_username(&self, username: &str) -> Result<()> {
        self.connection.execute(
            "INSERT INTO settings (key, value) VALUES ('github_username', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [username],
        )?;
        Ok(())
    }

    pub fn create_task(&mut self, username: &str, title: &str, description: &str) -> Result<Task> {
        let now = Utc::now().timestamp();
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO tasks (title, description, branch_name, created_at)
             VALUES (?1, ?2, '', ?3)",
            params![title, description, now],
        )?;
        let id = transaction.last_insert_rowid();
        let branch_name = branch_name(username, id, title);
        transaction.execute(
            "UPDATE tasks SET branch_name = ?1 WHERE id = ?2",
            params![branch_name, id],
        )?;
        transaction.commit()?;

        Ok(Task {
            id,
            title: title.to_owned(),
            description: description.to_owned(),
            branch_name,
            status: TaskStatus::Active,
            github_status: GithubStatus::NotPushed,
            pr_number: None,
            pr_url: None,
            created_at: now,
            completed_at: None,
            last_checked_at: None,
        })
    }

    pub fn tasks(&self) -> Result<Vec<Task>> {
        let mut statement = self.connection.prepare(
            "SELECT id, title, branch_name, status, github_status, pr_number, pr_url,
                    created_at, completed_at, last_checked_at, description
             FROM tasks
             ORDER BY CASE status WHEN 'active' THEN 0 ELSE 1 END, id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            let status: String = row.get(3)?;
            let github_status: String = row.get(4)?;
            Ok(Task {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(10)?,
                branch_name: row.get(2)?,
                status: TaskStatus::from_db(&status),
                github_status: GithubStatus::from_db(&github_status),
                pr_number: row.get(5)?,
                pr_url: row.get(6)?,
                created_at: row.get(7)?,
                completed_at: row.get(8)?,
                last_checked_at: row.get(9)?,
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("could not load tasks")
    }

    pub fn set_description(&self, task_id: i64, description: &str) -> Result<()> {
        self.connection.execute(
            "UPDATE tasks SET description = ?1 WHERE id = ?2",
            params![description, task_id],
        )?;
        Ok(())
    }

    pub fn apply_github_updates(&mut self, updates: &[GithubUpdate]) -> Result<()> {
        let checked_at = Utc::now().timestamp();
        let transaction = self.connection.transaction()?;
        for update in updates {
            let completed_at = (update.status == GithubStatus::Merged).then_some(checked_at);
            transaction.execute(
                "UPDATE tasks
                 SET github_status = ?1,
                     pr_number = ?2,
                     pr_url = ?3,
                     last_checked_at = ?4,
                     status = CASE WHEN ?5 IS NOT NULL THEN 'complete' ELSE status END,
                     completed_at = COALESCE(completed_at, ?5)
                 WHERE id = ?6",
                params![
                    update.status.as_str(),
                    update.pr_number,
                    update.pr_url,
                    checked_at,
                    completed_at,
                    update.task_id
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

pub fn branch_name(username: &str, id: i64, title: &str) -> String {
    let mut slug = String::new();
    let mut previous_was_separator = false;

    for character in title.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
            previous_was_separator = false;
        } else if !slug.is_empty() && !previous_was_separator {
            slug.push('-');
            previous_was_separator = true;
        }

        if slug.len() >= 48 {
            break;
        }
    }

    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "task" } else { slug };
    format!("{username}/{id}-{slug}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_safe_unique_branch_names() {
        assert_eq!(
            branch_name("icehunt", 42, "Add filtering & search!"),
            "icehunt/42-add-filtering-search"
        );
        assert_eq!(branch_name("icehunt", 7, "***"), "icehunt/7-task");
    }

    #[test]
    fn persists_tasks_and_completion() {
        let directory = tempfile::tempdir().unwrap();
        let mut database = Database::open(&directory.path().join("tasks.db")).unwrap();
        let task = database.create_task("icehunt", "Ship it", "").unwrap();

        database
            .apply_github_updates(&[GithubUpdate {
                task_id: task.id,
                status: GithubStatus::Merged,
                pr_number: Some(12),
                pr_url: Some("https://github.com/example/repo/pull/12".into()),
            }])
            .unwrap();

        let tasks = database.tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, TaskStatus::Complete);
        assert_eq!(tasks[0].github_status, GithubStatus::Merged);
        assert!(tasks[0].completed_at.is_some());
    }

    #[test]
    fn closed_pull_requests_do_not_complete_tasks() {
        let directory = tempfile::tempdir().unwrap();
        let mut database = Database::open(&directory.path().join("tasks.db")).unwrap();
        let task = database.create_task("icehunt", "Keep working", "").unwrap();

        database
            .apply_github_updates(&[GithubUpdate {
                task_id: task.id,
                status: GithubStatus::ClosedPr,
                pr_number: Some(13),
                pr_url: None,
            }])
            .unwrap();

        let tasks = database.tasks().unwrap();
        assert_eq!(tasks[0].status, TaskStatus::Active);
        assert!(tasks[0].completed_at.is_none());
    }

    #[test]
    fn persists_descriptions() {
        let directory = tempfile::tempdir().unwrap();
        let mut database = Database::open(&directory.path().join("tasks.db")).unwrap();
        let task = database
            .create_task("icehunt", "Document it", "Explain the setup")
            .unwrap();
        assert_eq!(
            database.tasks().unwrap()[0].description,
            "Explain the setup"
        );

        database.set_description(task.id, "Updated").unwrap();
        assert_eq!(database.tasks().unwrap()[0].description, "Updated");
    }

    #[test]
    fn migrates_databases_without_descriptions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tasks.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE tasks (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     title TEXT NOT NULL,
                     branch_name TEXT NOT NULL UNIQUE,
                     status TEXT NOT NULL DEFAULT 'active',
                     github_status TEXT NOT NULL DEFAULT 'not_pushed',
                     pr_number INTEGER,
                     pr_url TEXT,
                     created_at INTEGER NOT NULL,
                     completed_at INTEGER,
                     last_checked_at INTEGER
                 );
                 INSERT INTO tasks (title, branch_name, created_at)
                 VALUES ('Old task', 'task/1-old-task', 0);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        drop(connection);

        let database = Database::open(&path).unwrap();
        assert_eq!(database.tasks().unwrap()[0].description, "");
    }

    #[test]
    fn persists_github_username() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(&directory.path().join("tasks.db")).unwrap();

        assert_eq!(database.github_username().unwrap(), None);
        database.set_github_username("icehunt").unwrap();
        assert_eq!(
            database.github_username().unwrap().as_deref(),
            Some("icehunt")
        );
    }

    #[test]
    fn keeps_existing_branch_names_when_adding_username() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(&directory.path().join("tasks.db")).unwrap();
        database
            .connection
            .execute(
                "INSERT INTO tasks (title, branch_name, created_at) VALUES (?1, ?2, ?3)",
                params!["Existing task", "task/1-existing-task", 0],
            )
            .unwrap();

        database.set_github_username("icehunt").unwrap();

        assert_eq!(
            database.tasks().unwrap()[0].branch_name,
            "task/1-existing-task"
        );
    }
}
