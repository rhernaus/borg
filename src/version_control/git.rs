use anyhow::{Context, Result};
use async_trait::async_trait;
use git2::{Repository, BranchType, Signature, ObjectType};
use log::{debug, info};
use std::path::{Path, PathBuf};

use crate::core::error::BorgError;

/// Git manager trait for version control operations
#[async_trait]
pub trait GitManager: Send + Sync {
    /// Initialize a git repository
    async fn init_repository(&self, path: &Path) -> Result<()>;

    /// Create a new branch
    async fn create_branch(&self, branch_name: &str) -> Result<()>;

    /// Check out a branch
    async fn checkout_branch(&self, branch_name: &str) -> Result<()>;

    /// Add files to the staging area
    async fn add_files(&self, file_paths: &[&Path]) -> Result<()>;

    /// Commit changes
    async fn commit(&self, message: &str) -> Result<String>;

    /// Merge a branch into the current branch
    async fn merge_branch(&self, branch_name: &str) -> Result<()>;

    /// Delete a branch
    async fn delete_branch(&self, branch_name: &str) -> Result<()>;

    /// Get the current branch name
    async fn get_current_branch(&self) -> Result<String>;

    /// Check if a branch exists
    async fn branch_exists(&self, branch_name: &str) -> Result<bool>;

    /// Get the diff between two branches
    async fn get_diff(&self, from_branch: &str, to_branch: &str) -> Result<String>;

    /// Read a file from the repository
    async fn read_file(&self, file_path: &str) -> Result<String>;
}

/// Git manager implementation using libgit2
pub struct LibGitManager {
    /// Path to the repository
    repo_path: PathBuf,

    /// Author name for commits
    author_name: String,

    /// Author email for commits
    author_email: String,
}

impl LibGitManager {
    /// Create a new Git manager
    pub fn new<P: AsRef<Path>>(repo_path: P, author_name: &str, author_email: &str) -> Self {
        Self {
            repo_path: repo_path.as_ref().to_path_buf(),
            author_name: author_name.to_string(),
            author_email: author_email.to_string(),
        }
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
impl GitManager for LibGitManager {
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

        // Get HEAD commit to branch from
        let head = repo.head()
            .context("Failed to get repository HEAD")?;

        let commit = head.peel_to_commit()
            .context("Failed to peel HEAD to commit")?;

        // Create branch
        repo.branch(branch_name, &commit, false)
            .with_context(|| format!("Failed to create branch '{}'", branch_name))?;

        info!("Created branch '{}'", branch_name);
        Ok(())
    }

    async fn checkout_branch(&self, branch_name: &str) -> Result<()> {
        let repo = self.open_repo()?;

        // Find the branch
        let branch = repo.find_branch(branch_name, BranchType::Local)
            .with_context(|| format!("Failed to find branch '{}'", branch_name))?;

        let obj = branch.get().peel(ObjectType::Commit)
            .with_context(|| format!("Failed to peel branch '{}' to commit", branch_name))?;

        // Check out the branch - Using custom options rather than CheckoutOptions::new()
        let mut opts = git2::build::CheckoutBuilder::new();
        opts.force();

        repo.checkout_tree(&obj, Some(&mut opts))
            .with_context(|| format!("Failed to check out tree for branch '{}'", branch_name))?;

        repo.set_head(&format!("refs/heads/{}", branch_name))
            .with_context(|| format!("Failed to set HEAD to branch '{}'", branch_name))?;

        info!("Checked out branch '{}'", branch_name);
        Ok(())
    }

    async fn add_files(&self, file_paths: &[&Path]) -> Result<()> {
        let repo = self.open_repo()?;
        let mut index = repo.index()
            .context("Failed to get repository index")?;

        for file_path in file_paths {
            let rel_path = pathdiff::diff_paths(file_path, &self.repo_path)
                .with_context(|| format!("Failed to compute relative path for {:?}", file_path))?;

            index.add_path(&rel_path)
                .with_context(|| format!("Failed to add file {:?} to index", file_path))?;
        }

        index.write()
            .context("Failed to write index")?;

        debug!("Added {} files to the index", file_paths.len());
        Ok(())
    }

    async fn commit(&self, message: &str) -> Result<String> {
        let repo = self.open_repo()?;
        let mut index = repo.index()
            .context("Failed to get repository index")?;

        let oid = index.write_tree()
            .context("Failed to write index tree")?;

        let tree = repo.find_tree(oid)
            .context("Failed to find tree")?;

        let sig = self.create_signature()?;

        let head_result = repo.head();
        let parent_commits = match head_result {
            Ok(head) => {
                let head_commit = head.peel_to_commit()
                    .context("Failed to peel HEAD to commit")?;
                vec![head_commit]
            },
            Err(_) => {
                // No parent commit (initial commit)
                vec![]
            }
        };

        let parent_commits_refs: Vec<&git2::Commit> = parent_commits.iter().collect();

        let commit_oid = repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            message,
            &tree,
            &parent_commits_refs,
        ).context("Failed to create commit")?;

        info!("Created commit: {}", commit_oid);
        Ok(commit_oid.to_string())
    }

    async fn merge_branch(&self, branch_name: &str) -> Result<()> {
        let repo = self.open_repo()?;

        // Get the branch to merge
        let branch = repo.find_branch(branch_name, BranchType::Local)
            .with_context(|| format!("Failed to find branch '{}'", branch_name))?;

        let branch_ref = branch.get();
        let annotated_commit = repo.find_annotated_commit(branch_ref.target().unwrap())
            .with_context(|| format!("Failed to find annotated commit for branch '{}'", branch_name))?;

        // Perform merge analysis
        let (merge_analysis, _) = repo.merge_analysis(&[&annotated_commit])
            .context("Failed to perform merge analysis")?;

        if merge_analysis.is_up_to_date() {
            info!("Branch '{}' is already up-to-date with current branch", branch_name);
            return Ok(());
        }

        if merge_analysis.is_fast_forward() {
            // Fast-forward merge
            let branch_commit = repo.find_commit(branch_ref.target().unwrap())
                .with_context(|| format!("Failed to find commit for branch '{}'", branch_name))?;

            let head_ref = repo.head()?;
            let head_refname = head_ref.name().unwrap();

            repo.reference(head_refname, branch_ref.target().unwrap(), true,
                           &format!("Fast-forward merge of branch '{}'", branch_name))?;

            let obj = repo.find_object(branch_commit.id(), None)?;

            // Using CheckoutBuilder instead of CheckoutOptions
            let mut checkout_opts = git2::build::CheckoutBuilder::new();

            repo.checkout_tree(&obj, Some(&mut checkout_opts))?;

            info!("Fast-forward merged branch '{}'", branch_name);
        } else {
            // Normal merge
            let sig = self.create_signature()?;

            // Using a MergeOptions builder if available, otherwise use defaults
            let mut merge_opts = git2::MergeOptions::new();
            merge_opts.fail_on_conflict(false);

            // Using CheckoutBuilder instead of CheckoutOptions
            let mut checkout_opts = git2::build::CheckoutBuilder::new();
            checkout_opts.allow_conflicts(true);
            checkout_opts.force();

            repo.merge(&[&annotated_commit], Some(&mut merge_opts), Some(&mut checkout_opts))
                .with_context(|| format!("Failed to merge branch '{}'", branch_name))?;

            // Check for conflicts
            let mut index = repo.index()?;
            if index.has_conflicts() {
                // In a real implementation, we would handle conflicts more gracefully
                return Err(anyhow::anyhow!(BorgError::GitError(
                    format!("Merge conflicts detected when merging branch '{}'", branch_name)
                )));
            }

            // Create merge commit
            let oid = index.write_tree()?;
            let tree = repo.find_tree(oid)?;

            let head_commit = repo.head()?.peel_to_commit()?;
            let branch_commit = repo.find_commit(branch_ref.target().unwrap())?;

            repo.commit(
                Some("HEAD"),
                &sig,
                &sig,
                &format!("Merge branch '{}'", branch_name),
                &tree,
                &[&head_commit, &branch_commit],
            )?;

            // Clean up merge state
            repo.cleanup_state()?;

            info!("Merged branch '{}' with commit", branch_name);
        }

        Ok(())
    }

    async fn delete_branch(&self, branch_name: &str) -> Result<()> {
        let repo = self.open_repo()?;

        let mut branch = repo.find_branch(branch_name, BranchType::Local)
            .with_context(|| format!("Failed to find branch '{}'", branch_name))?;

        branch.delete()
            .with_context(|| format!("Failed to delete branch '{}'", branch_name))?;

        info!("Deleted branch '{}'", branch_name);
        Ok(())
    }

    async fn get_current_branch(&self) -> Result<String> {
        let repo = self.open_repo()?;

        let head = repo.head()
            .context("Failed to get repository HEAD")?;

        if !head.is_branch() {
            return Err(anyhow::anyhow!(BorgError::GitError(
                "HEAD is not pointing to a branch".to_string()
            )));
        }

        let branch_name = head.shorthand()
            .ok_or_else(|| anyhow::anyhow!(BorgError::GitError(
                "Failed to get branch shorthand".to_string()
            )))?;

        Ok(branch_name.to_string())
    }

    async fn branch_exists(&self, branch_name: &str) -> Result<bool> {
        let repo = self.open_repo()?;

        // Using a local variable to avoid lifetime issues
        let exists = match repo.find_branch(branch_name, BranchType::Local) {
            Ok(_) => true,
            Err(_) => false,
        };

        Ok(exists)
    }

    async fn get_diff(&self, from_branch: &str, to_branch: &str) -> Result<String> {
        let repo = self.open_repo()?;

        // Get commit for from_branch
        let from_branch_obj = repo.revparse_single(&format!("refs/heads/{}", from_branch))
            .with_context(|| format!("Failed to find branch '{}'", from_branch))?;

        let from_commit = from_branch_obj.peel_to_commit()
            .with_context(|| format!("Failed to peel '{}' to commit", from_branch))?;

        // Get commit for to_branch
        let to_branch_obj = repo.revparse_single(&format!("refs/heads/{}", to_branch))
            .with_context(|| format!("Failed to find branch '{}'", to_branch))?;

        let to_commit = to_branch_obj.peel_to_commit()
            .with_context(|| format!("Failed to peel '{}' to commit", to_branch))?;

        // Get trees for both commits
        let from_tree = from_commit.tree()
            .with_context(|| format!("Failed to get tree for '{}'", from_branch))?;

        let to_tree = to_commit.tree()
            .with_context(|| format!("Failed to get tree for '{}'", to_branch))?;

        // Create diff
        let mut diff_options = git2::DiffOptions::new();
        diff_options.context_lines(3);
        diff_options.patience(true);

        let diff = repo.diff_tree_to_tree(Some(&from_tree), Some(&to_tree), Some(&mut diff_options))
            .with_context(|| format!("Failed to diff '{}' to '{}'", from_branch, to_branch))?;

        // Generate diff stats
        let stats = diff.stats()
            .context("Failed to get diff stats")?;

        let mut diff_output = format!(
            "Diff between '{}' and '{}':\n{} files changed, {} insertions(+), {} deletions(-)\n\n",
            from_branch, to_branch,
            stats.files_changed(),
            stats.insertions(),
            stats.deletions()
        );

        // Format diff for readability
        let diff_format = git2::DiffFormat::Patch;
        diff.print(diff_format, |_delta, _hunk, line| {
            let content = std::str::from_utf8(line.content()).unwrap_or("");

            match line.origin() {
                '+' => diff_output.push_str(&format!("+{}", content)),
                '-' => diff_output.push_str(&format!("-{}", content)),
                _ => diff_output.push_str(&format!(" {}", content)),
            }

            true
        })?;

        Ok(diff_output)
    }

    async fn read_file(&self, file_path: &str) -> Result<String> {
        let path = self.repo_path.join(file_path);

        if !path.exists() {
            return Err(anyhow::anyhow!("File does not exist: {}", file_path));
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read file: {}", file_path))?;

        Ok(content)
    }
}