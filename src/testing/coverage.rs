use anyhow::{Context, Result};
use log::info;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::core::error::BorgError;

/// Coverage data for a specific file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCoverage {
    /// Path to the file
    pub file_path: String,

    /// Total number of lines in the file
    pub total_lines: usize,

    /// Number of lines covered by tests
    pub covered_lines: usize,

    /// Coverage percentage (0-100)
    pub coverage_percentage: f64,

    /// Lines that are covered (1-indexed)
    pub covered_line_numbers: Vec<usize>,

    /// Lines that aren't covered (1-indexed)
    pub uncovered_line_numbers: Vec<usize>,
}

/// Overall coverage report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageReport {
    /// Coverage data for individual files
    pub files: Vec<FileCoverage>,

    /// Overall coverage percentage
    pub total_coverage_percentage: f64,

    /// Total number of lines across all files
    pub total_lines: usize,

    /// Total number of covered lines across all files
    pub total_covered_lines: usize,

    /// Time taken to generate the report
    pub generation_time: Duration,
}

/// A tool for generating test coverage reports
pub struct CoverageReporter {
    /// Path to the workspace
    workspace: PathBuf,

    /// Path to store coverage data
    coverage_dir: PathBuf,
}

impl CoverageReporter {
    /// Create a new coverage reporter
    pub fn new<P: AsRef<Path>>(workspace: P) -> Result<Self> {
        let workspace_path = workspace.as_ref().to_path_buf();
        let coverage_dir = workspace_path.join("target").join("coverage");

        // Create coverage directory if it doesn't exist
        fs::create_dir_all(&coverage_dir).context(format!(
            "Failed to create coverage directory at {:?}",
            coverage_dir
        ))?;

        Ok(Self {
            workspace: workspace_path,
            coverage_dir,
        })
    }

    /// Generate a test coverage report
    pub async fn generate_report(&self, branch: &str) -> Result<CoverageReport> {
        info!("Generating test coverage report for branch {}", branch);
        let start_time = Instant::now();

        // Check if grcov is installed
        self.check_grcov()?;

        // Set environment variables for coverage collection
        let mut cmd = Command::new("cargo");
        cmd.current_dir(&self.workspace)
            .env("CARGO_INCREMENTAL", "0")
            .env("RUSTFLAGS", "-Cinstrument-coverage")
            .env(
                "LLVM_PROFILE_FILE",
                self.coverage_dir
                    .join("coverage-%p-%m.profraw")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("test")
            .arg("--all-features");

        // Run the tests with coverage instrumentation
        let output = cmd
            .output()
            .context("Failed to run tests with coverage instrumentation")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(BorgError::TestingError(format!(
                "Tests failed while generating coverage: {}",
                stderr
            ))));
        }

        // Generate coverage report with grcov
        let grcov_output = Command::new("grcov")
            .current_dir(&self.workspace)
            .arg(self.coverage_dir.to_string_lossy().to_string())
            .arg("--binary-path")
            .arg(
                self.workspace
                    .join("target")
                    .join("debug")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("-s")
            .arg(self.workspace.to_string_lossy().to_string())
            .arg("-t")
            .arg("lcov")
            .arg("--llvm")
            .arg("--branch")
            .arg("--ignore-not-existing")
            .arg("--ignore")
            .arg("/*")
            .arg("--ignore")
            .arg("tests/*")
            .arg("--ignore")
            .arg("target/*")
            .arg("-o")
            .arg(
                self.coverage_dir
                    .join("lcov.info")
                    .to_string_lossy()
                    .to_string(),
            )
            .output()
            .context("Failed to run grcov")?;

        if !grcov_output.status.success() {
            let stderr = String::from_utf8_lossy(&grcov_output.stderr);
            return Err(anyhow::anyhow!(BorgError::TestingError(format!(
                "grcov failed: {}",
                stderr
            ))));
        }

        // Parse the lcov.info file to extract coverage data
        let lcov_path = self.coverage_dir.join("lcov.info");
        let lcov_content = fs::read_to_string(&lcov_path)
            .context(format!("Failed to read lcov file at {:?}", lcov_path))?;

        // Parse the LCOV data
        let coverage_data = self.parse_lcov(&lcov_content)?;

        // Calculate overall statistics
        let mut total_lines = 0;
        let mut total_covered_lines = 0;

        for file in &coverage_data {
            total_lines += file.total_lines;
            total_covered_lines += file.covered_lines;
        }

        let total_coverage_percentage = if total_lines > 0 {
            (total_covered_lines as f64 / total_lines as f64) * 100.0
        } else {
            0.0
        };

        let generation_time = start_time.elapsed();

        let report = CoverageReport {
            files: coverage_data,
            total_coverage_percentage,
            total_lines,
            total_covered_lines,
            generation_time,
        };

        info!("Coverage report generated in {:?}", generation_time);
        info!(
            "Overall coverage: {:.2}% ({}/{} lines)",
            report.total_coverage_percentage, report.total_covered_lines, report.total_lines
        );

        Ok(report)
    }

    /// Check if grcov is installed
    fn check_grcov(&self) -> Result<()> {
        let status = Command::new("grcov").arg("--version").status();

        match status {
            Ok(status) if status.success() => Ok(()),
            _ => Err(anyhow::anyhow!(BorgError::TestingError(
                "grcov is not installed. Install it with: cargo install grcov".to_string()
            ))),
        }
    }

    /// Parse LCOV data from a string
    fn parse_lcov(&self, lcov_content: &str) -> Result<Vec<FileCoverage>> {
        let mut result = Vec::new();
        let mut current_file: Option<String> = None;
        let mut line_coverage: HashMap<usize, bool> = HashMap::new();
        let mut total_lines = 0;

        for line in lcov_content.lines() {
            if line.starts_with("SF:") {
                // Save the previous file if there is one
                if let Some(file) = &current_file {
                    if !line_coverage.is_empty() {
                        result.push(self.create_file_coverage(file, &line_coverage, total_lines));
                    }
                }

                // Start a new file
                current_file = Some(line.trim_start_matches("SF:").to_string());
                line_coverage.clear();
                total_lines = 0;
            } else if line.starts_with("DA:") {
                // Line coverage data
                let parts: Vec<&str> = line.trim_start_matches("DA:").split(',').collect();
                if parts.len() >= 2 {
                    if let Ok(line_num) = parts[0].parse::<usize>() {
                        let is_covered = parts[1] != "0";
                        line_coverage.insert(line_num, is_covered);
                        total_lines += 1;
                    }
                }
            }
        }

        // Don't forget the last file
        if let Some(file) = &current_file {
            if !line_coverage.is_empty() {
                result.push(self.create_file_coverage(file, &line_coverage, total_lines));
            }
        }

        Ok(result)
    }

    /// Create a FileCoverage struct from parsed data
    fn create_file_coverage(
        &self,
        file_path: &str,
        line_coverage: &HashMap<usize, bool>,
        total_lines: usize,
    ) -> FileCoverage {
        let mut covered_lines = 0;
        let mut covered_line_numbers = Vec::new();
        let mut uncovered_line_numbers = Vec::new();

        for (&line_num, &is_covered) in line_coverage {
            if is_covered {
                covered_lines += 1;
                covered_line_numbers.push(line_num);
            } else {
                uncovered_line_numbers.push(line_num);
            }
        }

        let coverage_percentage = if total_lines > 0 {
            (covered_lines as f64 / total_lines as f64) * 100.0
        } else {
            0.0
        };

        FileCoverage {
            file_path: file_path.to_string(),
            total_lines,
            covered_lines,
            coverage_percentage,
            covered_line_numbers,
            uncovered_line_numbers,
        }
    }

    /// Generate a human-readable report
    pub fn generate_report_markdown(&self, report: &CoverageReport) -> String {
        let mut output = String::new();

        // Overall summary
        output.push_str("# Test Coverage Report\n\n");
        output.push_str(&format!(
            "**Overall Coverage:** {:.2}%\n",
            report.total_coverage_percentage
        ));
        output.push_str(&format!(
            "**Lines Covered:** {}/{}\n",
            report.total_covered_lines, report.total_lines
        ));
        output.push_str(&format!(
            "**Generated in:** {:.2} seconds\n\n",
            report.generation_time.as_secs_f64()
        ));

        // File table
        output.push_str("## File Coverage\n\n");
        output.push_str("| File | Coverage | Lines Covered |\n");
        output.push_str("|------|----------|---------------|\n");

        // Sort files by coverage percentage (ascending)
        let mut sorted_files = report.files.clone();
        sorted_files.sort_by(|a, b| {
            a.coverage_percentage
                .partial_cmp(&b.coverage_percentage)
                .unwrap()
        });

        for file in &sorted_files {
            // Skip files with 100% coverage
            if file.coverage_percentage < 100.0 {
                output.push_str(&format!(
                    "| {} | {:.2}% | {}/{} |\n",
                    file.file_path, file.coverage_percentage, file.covered_lines, file.total_lines
                ));
            }
        }

        // Files with low coverage
        if let Some(worst_file) = sorted_files.first() {
            if worst_file.coverage_percentage < 80.0 {
                output.push_str("\n## Files Needing Attention\n\n");

                for file in &sorted_files {
                    if file.coverage_percentage < 80.0 {
                        output.push_str(&format!("### {}\n", file.file_path));
                        output.push_str(&format!(
                            "**Coverage:** {:.2}%\n\n",
                            file.coverage_percentage
                        ));

                        if !file.uncovered_line_numbers.is_empty() {
                            output.push_str("**Uncovered Lines:**\n");

                            // Group consecutive line numbers
                            let mut ranges = Vec::new();
                            let mut start = file.uncovered_line_numbers[0];
                            let mut end = start;

                            for &line in &file.uncovered_line_numbers[1..] {
                                if line == end + 1 {
                                    end = line;
                                } else {
                                    ranges.push((start, end));
                                    start = line;
                                    end = line;
                                }
                            }
                            ranges.push((start, end));

                            // Display ranges
                            for (start, end) in ranges {
                                if start == end {
                                    output.push_str(&format!("- Line {}\n", start));
                                } else {
                                    output.push_str(&format!("- Lines {}-{}\n", start, end));
                                }
                            }

                            output.push('\n');
                        }
                    }
                }
            }
        }

        output
    }
}
