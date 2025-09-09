use anyhow::Result;
use async_trait::async_trait;
use log::{info, warn};
use std::sync::Mutex;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use sysinfo::System;
use tokio;

use crate::core::error::BorgError;
use crate::resource_monitor::monitor::{ResourceLimits, ResourceMonitor, ResourceUsage};

/// System resource monitor implementation
pub struct SystemMonitor {
    /// System information
    system: System,

    /// Process ID to monitor
    pid: Option<u32>,

    /// Start time of the process
    start_time: Instant,

    /// Peak memory usage
    peak_memory_mb: f64,

    /// Shared peak memory usage (for thread-safe updates)
    shared_peak_memory: Option<Arc<Mutex<f64>>>,

    /// Whether monitoring is active
    is_monitoring: bool,

    /// Monitoring interval
    monitoring_interval: Option<Duration>,

    /// Monitoring flag for background task
    monitoring_flag: Option<Arc<AtomicBool>>,
}

impl SystemMonitor {
    /// Create a new system resource monitor
    pub fn new() -> Result<Self> {
        let mut system = System::new_all();
        system.refresh_all();

        let current_pid = std::process::id();

        Ok(Self {
            system,
            pid: Some(current_pid),
            start_time: Instant::now(),
            peak_memory_mb: 100.0, // Set a reasonable default value (100MB) instead of 0
            shared_peak_memory: None,
            is_monitoring: false,
            monitoring_interval: None,
            monitoring_flag: None,
        })
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

    /// Update peak memory if current memory usage is higher
    fn update_peak_memory(&mut self, memory_mb: f64) {
        if memory_mb > self.peak_memory_mb {
            self.peak_memory_mb = memory_mb;
        }
    }
}

#[async_trait]
impl ResourceMonitor for SystemMonitor {
    async fn get_resource_usage(&self) -> Result<ResourceUsage> {
        let process = self.get_process().ok_or_else(|| {
            anyhow::anyhow!(BorgError::ResourceLimitError(
                "Failed to get process information".to_string()
            ))
        })?;

        let memory_mb = process.memory() as f64 / 1024.0 / 1024.0;
        let cpu_percent = process.cpu_usage() as f64;

        // Update peak memory tracking has been moved to a separate method
        // We can't call it here since we don't have &mut self
        // A better approach would be to use interior mutability with Mutex or AtomicF64

        // Check if we have a shared peak memory value to update
        if let Some(shared_peak) = &self.shared_peak_memory {
            let mut peak = shared_peak.lock().unwrap();
            if memory_mb > *peak {
                *peak = memory_mb;
                // Also update our local copy
                let peak_memory_mb = memory_mb;

                // We need a way to update the self.peak_memory_mb field here
                // For now we'll use the shared value for everything
            }
        }

        let disk_usage = None; // would be implemented in a real system

        // Get the current peak memory - either from our field or the shared atomic
        let current_peak = if let Some(shared_peak) = &self.shared_peak_memory {
            *shared_peak.lock().unwrap()
        } else {
            self.peak_memory_mb
        };

        // Only consider memory critical if it's over 500MB or CPU is over 95%
        let is_critical = (memory_mb > 500.0) || cpu_percent > 95.0;

        if is_critical {
            warn!(
                "Critical resource usage detected: memory={:.2}MB, CPU={:.2}%",
                memory_mb, cpu_percent
            );
        }

        Ok(ResourceUsage {
            memory_usage_mb: memory_mb,
            peak_memory_usage_mb: current_peak,
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

        // Create a new system for the background task since System doesn't implement Clone
        let mut system = System::new_all();
        system.refresh_all();

        // Create shared peak memory tracker
        let shared_peak_memory = Arc::new(Mutex::new(self.peak_memory_mb));
        self.shared_peak_memory = Some(shared_peak_memory.clone());

        // Default to 500MB if peak memory is too low
        let max_memory_mb = if self.peak_memory_mb < 100.0 {
            500.0
        } else {
            self.peak_memory_mb * 1.5
        };
        // Always use 95% as max CPU percent (not tied to memory)
        let max_cpu_percent = 95.0;
        let interval = Duration::from_millis(interval_ms);
        let monitoring_flag = Arc::new(AtomicBool::new(true));
        let monitoring_flag_clone = monitoring_flag.clone();

        // Store the flag so we can signal the task to stop
        self.monitoring_flag = Some(monitoring_flag);

        // Launch a background task that periodically checks resource usage
        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);

            while monitoring_flag_clone.load(Ordering::SeqCst) {
                interval_timer.tick().await;

                // Refresh system information
                system.refresh_all();

                // Get current memory usage
                let used_memory = system.used_memory();
                let used_memory_mb = used_memory / 1024 / 1024; // Convert to MB

                // Update peak memory if current usage is higher
                {
                    let mut peak = shared_peak_memory.lock().unwrap();
                    if used_memory_mb as f64 > *peak {
                        *peak = used_memory_mb as f64;
                    }
                }

                // Get CPU usage using the global CPU info
                let cpu_usage = system.global_cpu_usage();

                // Check if we're exceeding limits
                let memory_exceeded =
                    if max_memory_mb > 0.0 && used_memory_mb > max_memory_mb as u64 {
                        warn!(
                            "Memory usage exceeded: {} MB (limit: {} MB)",
                            used_memory_mb, max_memory_mb
                        );
                        true
                    } else {
                        false
                    };

                let cpu_exceeded = if max_cpu_percent > 0.0 && cpu_usage > max_cpu_percent as f32 {
                    warn!(
                        "CPU usage exceeded: {:.1}% (limit: {}%)",
                        cpu_usage, max_cpu_percent
                    );
                    true
                } else {
                    false
                };

                // Log current usage at info level
                info!(
                    "Resource usage - Memory: {} MB, CPU: {:.1}%",
                    used_memory_mb, cpu_usage
                );

                // If any resource is exceeded, log at warning level
                if memory_exceeded || cpu_exceeded {
                    warn!("Resource limits exceeded");
                    // In a production system, this could trigger alerts or throttling
                }
            }

            info!("Resource monitoring task stopped");
        });

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

        // Signal the background task to stop
        if let Some(flag) = &self.monitoring_flag {
            flag.store(false, Ordering::SeqCst);
        }

        // Update our peak memory from the shared source
        if let Some(shared_peak) = &self.shared_peak_memory {
            self.peak_memory_mb = *shared_peak.lock().unwrap();
        }

        self.is_monitoring = false;
        self.monitoring_flag = None;
        self.shared_peak_memory = None;
        info!("Resource monitoring stopped");

        Ok(())
    }
}
