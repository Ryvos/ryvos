use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use cron::Schedule;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use ryvos_core::config::CronConfig;
use ryvos_core::event::EventBus;
use ryvos_core::goal::{CriterionType, Goal, SuccessCriterion};
use ryvos_core::types::{AgentEvent, SessionId};

use crate::AgentRuntime;

struct CronJob {
    name: String,
    schedule: Schedule,
    prompt: String,
    #[allow(dead_code)]
    channel: Option<String>,
    goal: Option<String>,
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
                        goal: job_config.goal.clone(),
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
                let delay = (fire_at - now).to_std().unwrap_or(Duration::from_secs(1));

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

                        // Use Director orchestration when goal is configured
                        let run_result = if let Some(ref goal_desc) = job.goal {
                            let goal = Goal {
                                description: goal_desc.clone(),
                                success_criteria: vec![SuccessCriterion {
                                    id: "llm_judge".into(),
                                    criterion_type: CriterionType::LlmJudge {
                                        prompt: format!("Did the agent achieve this goal: {}?", goal_desc),
                                    },
                                    weight: 1.0,
                                    description: "Goal achievement".into(),
                                }],
                                constraints: vec![],
                                success_threshold: 0.7,
                                version: 0,
                                metrics: Default::default(),
                            };
                            info!(job = %job.name, "Cron job using Director orchestration");
                            self.runtime.run_with_goal(&session_id, &job.prompt, Some(&goal)).await
                        } else {
                            self.runtime.run(&session_id, &job.prompt).await
                        };

                        match run_result {
                            Ok(response) => {
                                info!(job = %job.name, "Cron job completed");
                                self.event_bus.publish(AgentEvent::CronJobComplete {
                                    name: job.name.clone(),
                                    response,
                                    channel: job.channel.clone(),
                                });
                            }
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
