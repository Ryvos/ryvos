use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use cron::Schedule;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use ryvos_core::config::CronConfig;
use ryvos_core::event::EventBus;
use ryvos_core::types::{AgentEvent, SessionId};

use crate::AgentRuntime;

struct CronJob {
    name: String,
    schedule: Schedule,
    prompt: String,
    #[allow(dead_code)]
    channel: Option<String>,
}

/// Runs scheduled agent tasks based on cron expressions.
pub struct CronScheduler {
    jobs: Vec<CronJob>,
    runtime: Arc<AgentRuntime>,
    event_bus: Arc<EventBus>,
    cancel: CancellationToken,
}

impl CronScheduler {
    pub fn new(
        config: &CronConfig,
        runtime: Arc<AgentRuntime>,
        event_bus: Arc<EventBus>,
        cancel: CancellationToken,
    ) -> Self {
        let mut jobs = Vec::new();

        for job_config in &config.jobs {
            match Schedule::from_str(&job_config.schedule) {
                Ok(schedule) => {
                    jobs.push(CronJob {
                        name: job_config.name.clone(),
                        schedule,
                        prompt: job_config.prompt.clone(),
                        channel: job_config.channel.clone(),
                    });
                    info!(name = %job_config.name, schedule = %job_config.schedule, "Cron job registered");
                }
                Err(e) => {
                    warn!(
                        name = %job_config.name,
                        schedule = %job_config.schedule,
                        error = %e,
                        "Invalid cron expression, skipping job"
                    );
                }
            }
        }

        Self {
            jobs,
            runtime,
            event_bus,
            cancel,
        }
    }

    /// Run the scheduler loop. Blocks until cancelled.
    pub async fn run(&self) {
        if self.jobs.is_empty() {
            info!("No cron jobs configured, scheduler idle");
            self.cancel.cancelled().await;
            return;
        }

        info!(count = self.jobs.len(), "Cron scheduler started");

        loop {
            // Find the next job to fire
            let now = Utc::now();
            let mut next_fire: Option<(chrono::DateTime<Utc>, &CronJob)> = None;

            for job in &self.jobs {
                if let Some(next) = job.schedule.upcoming(Utc).next() {
                    if next_fire.is_none() || next < next_fire.unwrap().0 {
                        next_fire = Some((next, job));
                    }
                }
            }

            if let Some((fire_at, job)) = next_fire {
                let delay = (fire_at - now)
                    .to_std()
                    .unwrap_or(Duration::from_secs(1));

                info!(
                    job = %job.name,
                    fire_at = %fire_at.format("%H:%M:%S"),
                    delay_secs = delay.as_secs(),
                    "Next cron job scheduled"
                );

                tokio::select! {
                    _ = tokio::time::sleep(delay) => {
                        info!(job = %job.name, "Firing cron job");

                        self.event_bus.publish(AgentEvent::CronFired {
                            job_id: job.name.clone(),
                            prompt: job.prompt.clone(),
                        });

                        let session_id = SessionId::from_string(&format!("cron:{}", job.name));
                        match self.runtime.run(&session_id, &job.prompt).await {
                            Ok(_) => info!(job = %job.name, "Cron job completed"),
                            Err(e) => error!(job = %job.name, error = %e, "Cron job failed"),
                        }
                    }
                    _ = self.cancel.cancelled() => {
                        info!("Cron scheduler shutting down");
                        break;
                    }
                }
            } else {
                // No jobs have upcoming times, wait until cancelled
                self.cancel.cancelled().await;
                break;
            }
        }
    }
}
