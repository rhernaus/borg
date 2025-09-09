use anyhow::Result;
use async_trait::async_trait;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use sysinfo::{ProcessExt, System, SystemExt};

use crate::core::error::BorgError;

/// Resource usage information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUsage {
    /// Current memory usage in megabytes
    pub memory_usage_mb: f64,

    /// Peak memory usage in megabytes
    pub peak_memory_usage_mb: f64,

    /// Current CPU usage percentage
    pub cpu_usage_percent: f64,

    /// Current disk usage in megabytes
    pub disk_usage_mb: Option<f64>,

    /// Uptime in seconds
    pub uptime_seconds: u64,

    /// Whether any resource is above its threshold
    pub is_resource_critical: bool,
}

/// Resource monitor interface
#[async_trait]
pub trait ResourceMonitor: Send + Sync {
    /// Get current resource usage
    async fn get_resource_usage(&self) -> Result<ResourceUsage>;

    /// Check if resource usage is within safe limits
    async fn is_within_limits(&self, limits: &ResourceLimits) -> Result<bool>;

    /// Start monitoring resources
    async fn start_monitoring(&mut self, interval_ms: u64) -> Result<()>;

    /// Stop monitoring resources
    async fn stop_monitoring(&mut self) -> Result<()>;
}

/// Resource limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Maximum memory usage in megabytes
    pub max_memory_mb: f64,

    /// Maximum CPU usage percentage
    pub max_cpu_percent: f64,

    /// Maximum disk usage in megabytes
    pub max_disk_mb: Option<f64>,
}

/// System resource monitor implementation
pub struct SystemResourceMonitor {
    /// System information
    system: System,

    /// Process ID to monitor
    pid: Option<u32>,

    /// Start time of the process
    start_time: Instant,

    /// Peak memory usage
    peak_memory_mb: f64,

    /// Whether monitoring is active
    is_monitoring: bool,

    /// Monitoring interval
    monitoring_interval: Option<Duration>,
}

impl SystemResourceMonitor {
    /// Create a new system resource monitor
    pub fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();

        let current_pid = std::process::id();

        Self {
            system,
            pid: Some(current_pid),
            start_time: Instant::now(),
            peak_memory_mb: 0.0,
            is_monitoring: false,
            monitoring_interval: None,
        }
    }

    /// Refresh system information
    fn refresh(&mut self) {
        self.system.refresh_all();
    }

    /// Get current process
    fn get_process(&self) -> Option<&sysinfo::Process> {
        if let Some(pid) = self.pid {
            self.system.process(sysinfo::Pid::from(pid as usize))
        } else {
            None
        }
    }
}

#[async_trait]
impl ResourceMonitor for SystemResourceMonitor {
    async fn get_resource_usage(&self) -> Result<ResourceUsage> {
        let process = self.get_process().ok_or_else(|| {
            anyhow::anyhow!(BorgError::ResourceLimitError(
                "Failed to get process information".to_string()
            ))
        })?;

        let memory_mb = process.memory() as f64 / 1024.0 / 1024.0;
        let cpu_percent = process.cpu_usage() as f64;

        let disk_usage = None; // would be implemented in a real system

        let is_critical = memory_mb > self.peak_memory_mb * 1.5 || cpu_percent > 95.0;

        if is_critical {
            warn!(
                "Critical resource usage detected: memory={:.2}MB, CPU={:.2}%",
                memory_mb, cpu_percent
            );
        }

        Ok(ResourceUsage {
            memory_usage_mb: memory_mb,
            peak_memory_usage_mb: self.peak_memory_mb,
            cpu_usage_percent: cpu_percent,
            disk_usage_mb: disk_usage,
            uptime_seconds: self.start_time.elapsed().as_secs(),
            is_resource_critical: is_critical,
        })
    }

    async fn is_within_limits(&self, limits: &ResourceLimits) -> Result<bool> {
        let usage = self.get_resource_usage().await?;

        let memory_within_limit = usage.memory_usage_mb <= limits.max_memory_mb;
        let cpu_within_limit = usage.cpu_usage_percent <= limits.max_cpu_percent;

        let disk_within_limit = match (usage.disk_usage_mb, limits.max_disk_mb) {
            (Some(usage), Some(limit)) => usage <= limit,
            _ => true, // If we don't have disk usage info or limit, assume it's fine
        };

        let all_within_limits = memory_within_limit && cpu_within_limit && disk_within_limit;

        if !all_within_limits {
            warn!(
                "Resource limits exceeded: memory={:.2}/{:.2}MB, CPU={:.2}/{:.2}%",
                usage.memory_usage_mb,
                limits.max_memory_mb,
                usage.cpu_usage_percent,
                limits.max_cpu_percent
            );
        }

        Ok(all_within_limits)
    }

    async fn start_monitoring(&mut self, interval_ms: u64) -> Result<()> {
        if self.is_monitoring {
            return Ok(());
        }

        self.monitoring_interval = Some(Duration::from_millis(interval_ms));
        self.is_monitoring = true;

        // In a real implementation, this would start a background task to periodically check resources
        // For simplicity, we'll just log that monitoring has started
        info!(
            "Resource monitoring started with interval of {}ms",
            interval_ms
        );

        Ok(())
    }

    async fn stop_monitoring(&mut self) -> Result<()> {
        if !self.is_monitoring {
            return Ok(());
        }

        self.is_monitoring = false;
        info!("Resource monitoring stopped");

        Ok(())
    }
}
