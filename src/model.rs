use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Active,
    Complete,
}

impl TaskStatus {
    pub fn from_db(value: &str) -> Self {
        match value {
            "complete" => Self::Complete,
            _ => Self::Active,
        }
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "Active"),
            Self::Complete => write!(f, "Complete"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubStatus {
    Unknown,
    NotPushed,
    BranchPushed,
    DraftPr,
    OpenPr,
    ClosedPr,
    Merged,
}

impl GithubStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::NotPushed => "not_pushed",
            Self::BranchPushed => "branch_pushed",
            Self::DraftPr => "draft_pr",
            Self::OpenPr => "open_pr",
            Self::ClosedPr => "closed_pr",
            Self::Merged => "merged",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "not_pushed" => Self::NotPushed,
            "branch_pushed" => Self::BranchPushed,
            "draft_pr" => Self::DraftPr,
            "open_pr" => Self::OpenPr,
            "closed_pr" => Self::ClosedPr,
            "merged" => Self::Merged,
            _ => Self::Unknown,
        }
    }
}

impl fmt::Display for GithubStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Unknown => "Unknown",
            Self::NotPushed => "Not pushed",
            Self::BranchPushed => "Branch pushed",
            Self::DraftPr => "Draft PR",
            Self::OpenPr => "PR open",
            Self::ClosedPr => "PR closed",
            Self::Merged => "Merged",
        };
        write!(f, "{label}")
    }
}

#[derive(Debug, Clone)]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub branch_name: String,
    pub status: TaskStatus,
    pub github_status: GithubStatus,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub last_checked_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct GithubUpdate {
    pub task_id: i64,
    pub status: GithubStatus,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
}
