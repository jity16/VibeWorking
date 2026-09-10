use crate::models::{AgentSession, RetryJob};
use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryDecision { Send, Wait { delay_seconds: i64 }, Cancel, Escalate }

pub struct RetryController;

impl RetryController {
    pub fn new_job(session: &AgentSession, run_id: &str, overload_key: &str, now: DateTime<Utc>) -> RetryJob {
        RetryJob { id: Uuid::new_v4().to_string(), session_id: session.id.clone(), run_id: run_id.to_string(), overload_event_key: overload_key.to_string(), attempts: 0, max_attempts: 5, next_attempt_at: Some((now + Duration::seconds(30)).to_rfc3339()), total_deadline_at: (now + Duration::minutes(15)).to_rfc3339(), status: "scheduled".into(), last_error: None }
    }

    pub fn decide(job: &RetryJob, session: &AgentSession, now: DateTime<Utc>, input_ready: bool, approval_pending: bool, binding_verified: bool, user_stopped: bool) -> RetryDecision {
        if user_stopped || job.status == "cancelled" { return RetryDecision::Cancel; }
        if job.attempts >= job.max_attempts { return RetryDecision::Escalate; }
        let deadline = DateTime::parse_from_rfc3339(&job.total_deadline_at).map(|d| d.with_timezone(&Utc)).unwrap_or(now);
        if now >= deadline { return RetryDecision::Escalate; }
        if session.execution_status != "backoff" || session.control_mode != "automation" || session.attention != "none" || approval_pending || !input_ready || !binding_verified { return RetryDecision::Cancel; }
        if let Some(next) = &job.next_attempt_at {
            if let Ok(next) = DateTime::parse_from_rfc3339(next) { if now < next.with_timezone(&Utc) { return RetryDecision::Wait { delay_seconds: (next.with_timezone(&Utc) - now).num_seconds().max(1) }; } }
        }
        RetryDecision::Send
    }

    pub fn mark_sent(job: &mut RetryJob, now: DateTime<Utc>) { job.attempts += 1; job.status = "sent".into(); job.next_attempt_at = None; job.last_error = None; let _ = now; }
    pub fn mark_backoff(job: &mut RetryJob, now: DateTime<Utc>, error: impl Into<String>) { job.status = "scheduled".into(); job.last_error = Some(error.into()); let delay = jittered_delay(job.attempts); job.next_attempt_at = Some((now + Duration::seconds(delay)).to_rfc3339()); }
}

fn jittered_delay(attempt: i64) -> i64 {
    let base = 30_i64.saturating_mul(2_i64.saturating_pow(attempt.clamp(0, 8) as u32)).min(300);
    let jitter = rand::rng().random_range(0..=base / 5);
    (base + jitter).min(300)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session() -> AgentSession { AgentSession { id:"s".into(),project_id:"p".into(),task_id:Some("t".into()),provider:"codex".into(),display_name:"x".into(),provider_session_id:Some("thr".into()),process_id:Some(1),execution_status:"backoff".into(),connectivity_status:"connected".into(),control_mode:"automation".into(),attention:"none".into(),current_step:None,recent_activity:None,last_activity_at:None,terminal_bound:true,created_at:"".into(),updated_at:"".into() } }
    #[test] fn budget_escalates() { let now=Utc::now(); let mut j=RetryController::new_job(&session(),"r","e",now); j.attempts=5; assert_eq!(RetryController::decide(&j,&session(),now,true,false,true,false),RetryDecision::Escalate); }
    #[test] fn approval_cancels_queued_send() { let now=Utc::now(); let j=RetryController::new_job(&session(),"r","e",now); assert_eq!(RetryController::decide(&j,&session(),now+Duration::seconds(31),true,true,true,false),RetryDecision::Cancel); }
    #[test] fn waits_until_the_scheduled_moment_then_sends() {
        let now=Utc::now(); let j=RetryController::new_job(&session(),"r","e",now);
        assert!(matches!(RetryController::decide(&j,&session(),now,true,false,true,false),RetryDecision::Wait{..}));
        assert_eq!(RetryController::decide(&j,&session(),now+Duration::seconds(31),true,false,true,false),RetryDecision::Send);
    }
    #[test] fn unverified_terminal_cancels_instead_of_sending() { let now=Utc::now(); let j=RetryController::new_job(&session(),"r","e",now); assert_eq!(RetryController::decide(&j,&session(),now+Duration::seconds(31),true,false,false,false),RetryDecision::Cancel); }
}
