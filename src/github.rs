use std::collections::{HashMap, HashSet};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use crate::model::{GithubStatus, GithubUpdate, Task};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequest {
    head_ref_name: String,
    number: i64,
    state: String,
    is_draft: bool,
    merged_at: Option<String>,
    url: String,
}

pub fn validate_repository(repository: &str) -> Result<String> {
    validate_repository_format(repository)?;
    run_gh(&["auth", "status"]).context("GitHub CLI is not authenticated; run `gh auth login`")?;

    let output = run_gh(&[
        "repo",
        "view",
        repository,
        "--json",
        "nameWithOwner",
        "--jq",
        ".nameWithOwner",
    ])?;
    let canonical = output.trim();
    validate_repository_format(canonical)?;
    Ok(canonical.to_owned())
}

pub fn authenticated_username() -> Result<String> {
    let output = run_gh(&["api", "user", "--jq", ".login"])
        .context("could not determine the authenticated GitHub username")?;
    let username = output.trim();
    validate_username_format(username)?;
    Ok(username.to_owned())
}

pub fn synchronize(repository: &str, tasks: &[Task]) -> Result<Vec<GithubUpdate>> {
    validate_repository_format(repository)?;
    if tasks.is_empty() {
        return Ok(Vec::new());
    }

    let prs_json = run_gh(&[
        "pr",
        "list",
        "--repo",
        repository,
        "--state",
        "all",
        "--limit",
        "1000",
        "--json",
        "headRefName,number,state,isDraft,mergedAt,url",
    ])?;
    let pull_requests: Vec<PullRequest> =
        serde_json::from_str(&prs_json).context("could not parse pull requests from `gh`")?;

    let endpoint = format!("repos/{repository}/branches?per_page=100");
    let branches_output = run_gh(&["api", "--paginate", &endpoint, "--jq", ".[].name"])?;
    let branches: HashSet<&str> = branches_output.lines().collect();

    Ok(reconcile(tasks, pull_requests, &branches))
}

fn reconcile(
    tasks: &[Task],
    pull_requests: Vec<PullRequest>,
    branches: &HashSet<&str>,
) -> Vec<GithubUpdate> {
    let mut by_branch: HashMap<String, PullRequest> = HashMap::new();
    for pull_request in pull_requests {
        by_branch
            .entry(pull_request.head_ref_name.clone())
            .and_modify(|current| {
                if pull_request.merged_at.is_some() && current.merged_at.is_none() {
                    *current = pull_request.clone();
                }
            })
            .or_insert(pull_request);
    }

    tasks
        .iter()
        .map(|task| {
            let pull_request = by_branch.get(&task.branch_name);
            let status = match pull_request {
                Some(pr) if pr.merged_at.is_some() || pr.state == "MERGED" => GithubStatus::Merged,
                Some(pr) if pr.is_draft && pr.state == "OPEN" => GithubStatus::DraftPr,
                Some(pr) if pr.state == "OPEN" => GithubStatus::OpenPr,
                Some(_) => GithubStatus::ClosedPr,
                None if branches.contains(task.branch_name.as_str()) => GithubStatus::BranchPushed,
                None => GithubStatus::NotPushed,
            };
            GithubUpdate {
                task_id: task.id,
                status,
                pr_number: pull_request.map(|pr| pr.number),
                pr_url: pull_request.map(|pr| pr.url.clone()),
            }
        })
        .collect()
}

fn validate_repository_format(repository: &str) -> Result<()> {
    let mut parts = repository.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if owner.is_empty()
        || name.is_empty()
        || parts.next().is_some()
        || !owner.chars().all(valid_repository_character)
        || !name.chars().all(valid_repository_character)
    {
        bail!("repository must use the owner/name format");
    }
    Ok(())
}

fn valid_repository_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
}

fn validate_username_format(username: &str) -> Result<()> {
    if username.is_empty()
        || username.len() > 39
        || username.starts_with('-')
        || username.ends_with('-')
        || !username
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        bail!("GitHub returned an invalid username");
    }
    Ok(())
}

fn run_gh(arguments: &[&str]) -> Result<String> {
    let output = Command::new("gh")
        .args(arguments)
        .output()
        .context("could not run `gh`; install the GitHub CLI first")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(anyhow!(if stderr.is_empty() {
            format!("`gh {}` failed", arguments.join(" "))
        } else {
            stderr
        }));
    }
    String::from_utf8(output.stdout).context("`gh` returned invalid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TaskStatus;

    fn task(id: i64, branch: &str) -> Task {
        Task {
            id,
            title: "Task".into(),
            branch_name: branch.into(),
            status: TaskStatus::Active,
            github_status: GithubStatus::Unknown,
            pr_number: None,
            pr_url: None,
            created_at: 0,
            completed_at: None,
            last_checked_at: None,
        }
    }

    #[test]
    fn reconciles_branch_and_pr_states() {
        let tasks = vec![
            task(1, "task/1-local"),
            task(2, "task/2-open"),
            task(3, "task/3-done"),
        ];
        let json = r#"[
            {"headRefName":"task/2-open","number":2,"state":"OPEN","isDraft":false,"mergedAt":null,"url":"https://example/2"},
            {"headRefName":"task/3-done","number":3,"state":"MERGED","isDraft":false,"mergedAt":"2026-01-01T00:00:00Z","url":"https://example/3"}
        ]"#;
        let prs: Vec<PullRequest> = serde_json::from_str(json).unwrap();
        let updates = reconcile(&tasks, prs, &HashSet::new());

        assert_eq!(updates[0].status, GithubStatus::NotPushed);
        assert_eq!(updates[1].status, GithubStatus::OpenPr);
        assert_eq!(updates[2].status, GithubStatus::Merged);
    }

    #[test]
    fn rejects_invalid_repository_names() {
        assert!(validate_repository_format("owner/repo").is_ok());
        assert!(validate_repository_format("repo").is_err());
        assert!(validate_repository_format("owner/repo/extra").is_err());
    }

    #[test]
    fn validates_github_usernames() {
        assert!(validate_username_format("icehunt").is_ok());
        assert!(validate_username_format("octo-cat-42").is_ok());
        assert!(validate_username_format("").is_err());
        assert!(validate_username_format("-invalid").is_err());
        assert!(validate_username_format("invalid/name").is_err());
    }
}
