use anyhow::{Context, Result};
use async_trait::async_trait;
use git2::{Repository, BranchType, Signature, ObjectType};
use log::{info, warn};
use std::fs;
use std::path::{Path, PathBuf};

use crate::core::error::BorgError;
use crate::version_control::git::GitManager;

/// Git implementation using libgit2
pub struct GitImplementation {
    /// Path to the repository
    repo_path: PathBuf,

    /// Author name for commits
    author_name: String,

    /// Author email for commits
    author_email: String,
}

impl GitImplementation {
    /// Create a new Git implementation
    pub fn new<P: AsRef<Path>>(repo_path: P) -> Result<Self> {
        Ok(Self {
            repo_path: repo_path.as_ref().to_path_buf(),
            author_name: "Borg Agent".to_string(),
            author_email: "borg@example.com".to_string(),
        })
    }

    /// Open the repository
    fn open_repo(&self) -> Result<Repository> {
        let repo = Repository::open(&self.repo_path)
            .with_context(|| format!("Failed to open Git repository at {:?}", self.repo_path))?;

        Ok(repo)
    }

    /// Create a signature for commits
    fn create_signature(&self) -> Result<Signature<'static>> {
        let sig = Signature::now(&self.author_name, &self.author_email)
            .context("Failed to create Git signature")?;

        Ok(sig)
    }
}

#[async_trait]
impl GitManager for GitImplementation {
    async fn init_repository(&self, path: &Path) -> Result<()> {
        if path.exists() && path.is_dir() {
            if path.join(".git").exists() {
                info!("Git repository already exists at {:?}", path);
                return Ok(());
            }
        } else {
            std::fs::create_dir_all(path)
                .with_context(|| format!("Failed to create directory {:?}", path))?;
        }

        Repository::init(path)
            .with_context(|| format!("Failed to initialize Git repository at {:?}", path))?;

        info!("Initialized Git repository at {:?}", path);
        Ok(())
    }

    async fn create_branch(&self, branch_name: &str) -> Result<()> {
        let repo = self.open_repo()?;

        // Get HEAD commit
        let head = repo.head()?;
        let commit = head.peel_to_commit()?;

        // Create branch
        repo.branch(branch_name, &commit, false)
            .with_context(|| format!("Failed to create branch: {}", branch_name))?;

        info!("Created branch: {}", branch_name);
        Ok(())
    }

    async fn checkout_branch(&self, branch_name: &str) -> Result<()> {
        let repo = self.open_repo()?;

        // Find branch
        let branch = repo.find_branch(branch_name, BranchType::Local)
            .with_context(|| format!("Failed to find branch: {}", branch_name))?;

        // Get branch reference
        let branch_ref = branch.get();

        // Checkout branch
        let obj = branch_ref.peel(ObjectType::Any)?;
        repo.checkout_tree(&obj, None)
            .with_context(|| format!("Failed to checkout tree for branch: {}", branch_name))?;

        // Set HEAD to branch
        repo.set_head(branch_ref.name().unwrap())
            .with_context(|| format!("Failed to set HEAD to branch: {}", branch_name))?;

        info!("Checked out branch: {}", branch_name);
        Ok(())
    }

    async fn add_files(&self, file_paths: &[&Path]) -> Result<()> {
        let repo = self.open_repo()?;
        let mut index = repo.index()?;

        for path in file_paths {
            // Convert to relative path if needed
            let rel_path = if path.is_absolute() {
                path.strip_prefix(&self.repo_path)
                    .with_context(|| format!("Path is outside repository: {:?}", path))?
            } else {
                path
            };

            index.add_path(rel_path)
                .with_context(|| format!("Failed to add file to index: {:?}", rel_path))?;
        }

        index.write()?;

        info!("Added {} files to index", file_paths.len());
        Ok(())
    }

    async fn commit(&self, message: &str) -> Result<String> {
        let repo = self.open_repo()?;
        let signature = self.create_signature()?;
        let mut index = repo.index()?;

        // Write index to tree
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;

        // Get parent commit
        let head = match repo.head() {
            Ok(head) => Some(head.peel_to_commit()?),
            Err(_) => None,
        };

        let parents = match head {
            Some(ref commit) => vec![commit],
            None => vec![],
        };

        // Create commit
        let commit_id = repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parents,
        )?;

        info!("Created commit: {}", commit_id);
        Ok(commit_id.to_string())
    }

    async fn merge_branch(&self, branch_name: &str) -> Result<()> {
        let repo = self.open_repo()?;
        let signature = self.create_signature()?;

        // Find branch to merge
        let branch = repo.find_branch(branch_name, BranchType::Local)
            .with_context(|| format!("Failed to find branch: {}", branch_name))?;

        // Get branch commit
        let branch_commit = branch.get().peel_to_commit()?;

        // Get current branch
        let head = repo.head()?;
        let head_commit = head.peel_to_commit()?;

        // Create merge
        let merge_base = repo.merge_base(head_commit.id(), branch_commit.id())?;
        let _ancestor = repo.find_commit(merge_base)?;

        // For now, we'll do a simple fast-forward merge if possible
        // A more sophisticated implementation would handle non-fast-forward merges

        // Create merge commit
        let tree_id = repo.index()?.write_tree()?;
        let tree = repo.find_tree(tree_id)?;

        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            &format!("Merge branch '{}'", branch_name),
            &tree,
            &[&head_commit, &branch_commit],
        )?;

        info!("Merged branch: {}", branch_name);
        Ok(())
    }

    async fn delete_branch(&self, branch_name: &str) -> Result<()> {
        let repo = self.open_repo()?;

        let mut branch = repo.find_branch(branch_name, BranchType::Local)
            .with_context(|| format!("Failed to find branch: {}", branch_name))?;

        branch.delete()
            .with_context(|| format!("Failed to delete branch: {}", branch_name))?;

        info!("Deleted branch: {}", branch_name);
        Ok(())
    }

    async fn get_current_branch(&self) -> Result<String> {
        let repo = self.open_repo()?;

        let head = repo.head()?;
        if !head.is_branch() {
            return Err(anyhow::anyhow!(BorgError::GitError(
                "HEAD is not a branch".to_string()
            )));
        }

        let branch_name = head.shorthand().ok_or_else(|| anyhow::anyhow!(BorgError::GitError(
            "Failed to get branch name".to_string()
        )))?;

        Ok(branch_name.to_string())
    }

    async fn branch_exists(&self, branch_name: &str) -> Result<bool> {
        let repo = self.open_repo()?;

        let exists = repo.find_branch(branch_name, BranchType::Local).is_ok();
        Ok(exists)
    }

    async fn get_diff(&self, from_branch: &str, to_branch: &str) -> Result<String> {
        let repo = self.open_repo()?;

        // Get commits for both branches
        let from_branch_ref = repo.find_branch(from_branch, BranchType::Local)?;
        let to_branch_ref = repo.find_branch(to_branch, BranchType::Local)?;

        let from_commit = from_branch_ref.get().peel_to_commit()?;
        let to_commit = to_branch_ref.get().peel_to_commit()?;

        // Get trees for both commits
        let from_tree = from_commit.tree()?;
        let to_tree = to_commit.tree()?;

        // Create diff
        let diff = repo.diff_tree_to_tree(Some(&from_tree), Some(&to_tree), None)?;

        // Convert diff to string
        let mut diff_text = String::new();
        diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
            if let Ok(text) = std::str::from_utf8(line.content()) {
                diff_text.push_str(text);
            }
            true
        })?;

        Ok(diff_text)
    }

    async fn read_file(&self, file_path: &str) -> Result<String> {
        let path = self.repo_path.join(file_path);

        if !path.exists() {
            return Err(anyhow::anyhow!(BorgError::GitError(format!(
                "File does not exist: {}", file_path
            ))));
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read file: {}", file_path))?;

        Ok(content)
    }
}