//! Domain events broadcast to the `/api/v1/events` WS stream. The http adapter
//! maps these onto the `shared::events::EventEnvelope` wire shape.

use my_supervisor_core::domain::{JobRunId, ProcessState};

#[derive(Debug, Clone)]
pub enum DomainEvent {
    ProcessStateChanged {
        name: String,
        from: ProcessState,
        to: ProcessState,
    },
    JobRunScheduled {
        name: String,
        run_id: JobRunId,
    },
    JobRunStarted {
        name: String,
        run_id: JobRunId,
    },
    JobRunSucceeded {
        name: String,
        run_id: JobRunId,
        exit_code: i32,
    },
    JobRunFailed {
        name: String,
        run_id: JobRunId,
        exit_code: Option<i32>,
    },
    JobRunSkipped {
        name: String,
        run_id: JobRunId,
        reason: String,
    },
}
