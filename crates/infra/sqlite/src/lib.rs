//! `my-supervisor-infra-sqlite` — `StateRepository` + `JobRepository` over
//! SQLite (WAL). One `SqliteStore` implements both; the host injects it into
//! both `AppDeps` slots.

mod repr;

use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

use my_supervisor_core::domain::{
    Job, JobId, JobRun, JobRunId, JobRunState, LifecycleMode, LogRetention, ManagementMode,
    ProcessSpec, RestartPolicy, ShutdownPolicy, ShutdownSignal,
};
use my_supervisor_core::ports::error::RepoError;
use my_supervisor_core::ports::{JobRepository, StateRepository};

use repr::{TriggerRepr, TriggeredByRepr};

fn backend<E: std::fmt::Display>(e: E) -> RepoError {
    RepoError::Backend(e.to_string())
}

fn dt_to_str(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

fn str_to_dt(s: &str) -> Result<DateTime<Utc>, RepoError> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(backend)
}

fn opt_dt_to_str(dt: &Option<DateTime<Utc>>) -> Option<String> {
    dt.as_ref().map(dt_to_str)
}

fn opt_str_to_dt(s: Option<String>) -> Result<Option<DateTime<Utc>>, RepoError> {
    s.map(|s| str_to_dt(&s)).transpose()
}

fn json_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn run_state_to_str(s: JobRunState) -> &'static str {
    match s {
        JobRunState::Pending => "pending",
        JobRunState::Running => "running",
        JobRunState::Succeeded => "succeeded",
        JobRunState::Failed => "failed",
        JobRunState::Cancelled => "cancelled",
        JobRunState::Skipped => "skipped",
    }
}

fn str_to_run_state(s: &str) -> JobRunState {
    match s {
        "running" => JobRunState::Running,
        "succeeded" => JobRunState::Succeeded,
        "failed" => JobRunState::Failed,
        "cancelled" => JobRunState::Cancelled,
        "skipped" => JobRunState::Skipped,
        _ => JobRunState::Pending,
    }
}

pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    /// Open (creating if missing) the SQLite database at `path` and ensure schema.
    pub async fn connect(path: impl AsRef<Path>) -> Result<Self, RepoError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .map_err(backend)?;
        let store = SqliteStore { pool };
        store.migrate().await?;
        Ok(store)
    }

    /// In-memory store for tests / ephemeral hosts.
    pub async fn connect_in_memory() -> Result<Self, RepoError> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:").map_err(backend)?;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(backend)?;
        let store = SqliteStore { pool };
        store.migrate().await?;
        Ok(store)
    }

    async fn migrate(&self) -> Result<(), RepoError> {
        let ddl = r#"
            CREATE TABLE IF NOT EXISTS process_specs (
                name          TEXT PRIMARY KEY,
                command       TEXT NOT NULL,
                args          TEXT NOT NULL,
                cwd           TEXT,
                env           TEXT NOT NULL,
                mode          TEXT NOT NULL,
                unit_name     TEXT,
                lifecycle     TEXT NOT NULL,
                autostart     INTEGER NOT NULL,
                restart_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS jobs (
                name                  TEXT PRIMARY KEY,
                id                    TEXT NOT NULL,
                command               TEXT NOT NULL,
                args                  TEXT NOT NULL,
                cwd                   TEXT,
                env                   TEXT NOT NULL,
                trigger               TEXT NOT NULL,
                on_overlap            TEXT NOT NULL,
                on_dependency_failure TEXT NOT NULL,
                timeout_sec           INTEGER
            );
            CREATE TABLE IF NOT EXISTS job_runs (
                run_id       TEXT PRIMARY KEY,
                job_name     TEXT NOT NULL,
                triggered_by TEXT NOT NULL,
                scheduled_at TEXT NOT NULL,
                started_at   TEXT,
                ended_at     TEXT,
                exit_code    INTEGER,
                state        TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_job_runs_job
                ON job_runs(job_name, scheduled_at DESC);
        "#;
        for statement in ddl.split(';') {
            let trimmed = statement.trim();
            if trimmed.is_empty() {
                continue;
            }
            sqlx::query(trimmed)
                .execute(&self.pool)
                .await
                .map_err(backend)?;
        }
        self.ensure_process_spec_column("restart_enabled", "INTEGER NOT NULL DEFAULT 1")
            .await?;
        self.ensure_process_spec_column("restart_max_retries", "INTEGER")
            .await?;
        self.ensure_process_spec_column(
            "restart_backoff_initial_ms",
            "INTEGER NOT NULL DEFAULT 1000",
        )
        .await?;
        self.ensure_process_spec_column(
            "restart_backoff_max_ms",
            "INTEGER NOT NULL DEFAULT 60000",
        )
        .await?;
        self.ensure_process_spec_column(
            "restart_backoff_multiplier",
            "INTEGER NOT NULL DEFAULT 2",
        )
        .await?;
        self.ensure_process_spec_column("restart_jitter", "INTEGER NOT NULL DEFAULT 1")
            .await?;
        self.ensure_process_spec_column(
            "restart_reset_after_ms",
            "INTEGER NOT NULL DEFAULT 60000",
        )
        .await?;
        self.ensure_process_spec_column("runtime_process_id", "TEXT")
            .await?;
        self.ensure_process_spec_column("runtime_pid", "INTEGER")
            .await?;
        self.ensure_process_spec_column("runtime_started_at", "TEXT")
            .await?;
        self.ensure_process_spec_column("shutdown_signal", "TEXT NOT NULL DEFAULT 'term'")
            .await?;
        self.ensure_process_spec_column(
            "shutdown_grace_period_ms",
            "INTEGER NOT NULL DEFAULT 10000",
        )
        .await?;
        self.ensure_table_column("jobs", "log_retention_max_runs", "INTEGER")
            .await?;
        self.ensure_table_column("jobs", "log_retention_max_age_days", "INTEGER")
            .await?;
        Ok(())
    }

    async fn ensure_process_spec_column(
        &self,
        column_name: &str,
        definition: &str,
    ) -> Result<(), RepoError> {
        self.ensure_table_column("process_specs", column_name, definition)
            .await
    }

    async fn ensure_table_column(
        &self,
        table_name: &str,
        column_name: &str,
        definition: &str,
    ) -> Result<(), RepoError> {
        let columns = sqlx::query(&format!("PRAGMA table_info({table_name})"))
            .fetch_all(&self.pool)
            .await
            .map_err(backend)?;
        let exists = columns.iter().any(|row| {
            row.try_get::<String, _>("name")
                .map(|name| name == column_name)
                .unwrap_or(false)
        });
        if !exists {
            let statement = format!(
                "ALTER TABLE {table_name} ADD COLUMN {column_name} {definition}"
            );
            sqlx::query(&statement)
                .execute(&self.pool)
                .await
                .map_err(backend)?;
        }
        Ok(())
    }
}

fn parse_json_vec(s: &str) -> Vec<String> {
    serde_json::from_str(s).unwrap_or_default()
}

fn parse_json_map(s: &str) -> BTreeMap<String, String> {
    serde_json::from_str(s).unwrap_or_default()
}

fn spec_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<ProcessSpec, RepoError> {
    let mode: String = row.try_get("mode").map_err(backend)?;
    let unit_name: Option<String> = row.try_get("unit_name").map_err(backend)?;
    let management_mode = match mode.as_str() {
        "system_registered" => ManagementMode::SystemRegistered {
            unit_name: unit_name.unwrap_or_default(),
        },
        _ => ManagementMode::Direct,
    };
    let lifecycle: String = row.try_get("lifecycle").map_err(backend)?;
    let args: String = row.try_get("args").map_err(backend)?;
    let env: String = row.try_get("env").map_err(backend)?;
    let cwd: Option<String> = row.try_get("cwd").map_err(backend)?;
    let autostart: i64 = row.try_get("autostart").map_err(backend)?;
    let restart_enabled: i64 = row.try_get("restart_enabled").map_err(backend)?;
    let restart_max_retries: Option<i64> =
        row.try_get("restart_max_retries").map_err(backend)?;
    let restart_backoff_initial_ms: i64 = row
        .try_get("restart_backoff_initial_ms")
        .map_err(backend)?;
    let restart_backoff_max_ms: i64 = row
        .try_get("restart_backoff_max_ms")
        .map_err(backend)?;
    let restart_backoff_multiplier: i64 = row
        .try_get("restart_backoff_multiplier")
        .map_err(backend)?;
    let restart_jitter: i64 = row.try_get("restart_jitter").map_err(backend)?;
    let restart_reset_after_ms: i64 = row
        .try_get("restart_reset_after_ms")
        .map_err(backend)?;
    let shutdown_signal: String = row.try_get("shutdown_signal").map_err(backend)?;
    let shutdown_grace_period_ms: i64 = row
        .try_get("shutdown_grace_period_ms")
        .map_err(backend)?;

    Ok(ProcessSpec {
        name: row.try_get("name").map_err(backend)?,
        command: row.try_get("command").map_err(backend)?,
        args: parse_json_vec(&args),
        cwd: cwd.map(std::path::PathBuf::from),
        env: parse_json_map(&env),
        management_mode,
        lifecycle: if lifecycle == "detached" {
            LifecycleMode::Detached
        } else {
            LifecycleMode::Tied
        },
        autostart: autostart != 0,
        restart: RestartPolicy {
            enabled: restart_enabled != 0,
            max_retries: restart_max_retries.map(|value| value.max(0) as u32),
            backoff_initial: std::time::Duration::from_millis(
                restart_backoff_initial_ms.max(0) as u64,
            ),
            backoff_max: std::time::Duration::from_millis(
                restart_backoff_max_ms.max(0) as u64,
            ),
            backoff_multiplier: restart_backoff_multiplier.max(1) as u32,
            jitter: restart_jitter != 0,
            reset_after: std::time::Duration::from_millis(
                restart_reset_after_ms.max(0) as u64,
            ),
        },
        shutdown: ShutdownPolicy {
            signal: match shutdown_signal.as_str() {
                "int" => ShutdownSignal::Int,
                "kill" => ShutdownSignal::Kill,
                _ => ShutdownSignal::Term,
            },
            grace_period: std::time::Duration::from_millis(
                shutdown_grace_period_ms.max(0) as u64,
            ),
        },
    })
}

fn job_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Job, RepoError> {
    let id: String = row.try_get("id").map_err(backend)?;
    let args: String = row.try_get("args").map_err(backend)?;
    let env: String = row.try_get("env").map_err(backend)?;
    let cwd: Option<String> = row.try_get("cwd").map_err(backend)?;
    let trigger: String = row.try_get("trigger").map_err(backend)?;
    let on_overlap: String = row.try_get("on_overlap").map_err(backend)?;
    let on_dep: String = row.try_get("on_dependency_failure").map_err(backend)?;
    let timeout: Option<i64> = row.try_get("timeout_sec").map_err(backend)?;
    let log_retention_max_runs: Option<i64> =
        row.try_get("log_retention_max_runs").map_err(backend)?;
    let log_retention_max_age_days: Option<i64> = row
        .try_get("log_retention_max_age_days")
        .map_err(backend)?;

    let trigger_repr: TriggerRepr = serde_json::from_str(&trigger).map_err(backend)?;

    Ok(Job {
        id: JobId(uuid::Uuid::parse_str(&id).unwrap_or_default()),
        name: row.try_get("name").map_err(backend)?,
        command: row.try_get("command").map_err(backend)?,
        args: parse_json_vec(&args),
        cwd: cwd.map(std::path::PathBuf::from),
        env: parse_json_map(&env),
        trigger: trigger_repr.into(),
        on_overlap: match on_overlap.as_str() {
            "queue" => my_supervisor_core::domain::OverlapPolicy::Queue,
            "parallel" => my_supervisor_core::domain::OverlapPolicy::Parallel,
            _ => my_supervisor_core::domain::OverlapPolicy::Skip,
        },
        on_dependency_failure: match on_dep.as_str() {
            "run_anyway" => my_supervisor_core::domain::DependencyFailurePolicy::RunAnyway,
            _ => my_supervisor_core::domain::DependencyFailurePolicy::Skip,
        },
        timeout: timeout.map(|s| std::time::Duration::from_secs(s as u64)),
        log_retention: LogRetention {
            max_runs: log_retention_max_runs.map(|value| value.max(0) as u32),
            max_age_days: log_retention_max_age_days.map(|value| value.max(0) as u32),
        },
    })
}

fn run_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<JobRun, RepoError> {
    let run_id: String = row.try_get("run_id").map_err(backend)?;
    let triggered_by: String = row.try_get("triggered_by").map_err(backend)?;
    let scheduled_at: String = row.try_get("scheduled_at").map_err(backend)?;
    let started_at: Option<String> = row.try_get("started_at").map_err(backend)?;
    let ended_at: Option<String> = row.try_get("ended_at").map_err(backend)?;
    let exit_code: Option<i64> = row.try_get("exit_code").map_err(backend)?;
    let state: String = row.try_get("state").map_err(backend)?;

    let triggered_repr: TriggeredByRepr = serde_json::from_str(&triggered_by).map_err(backend)?;

    Ok(JobRun {
        run_id: JobRunId(uuid::Uuid::parse_str(&run_id).unwrap_or_default()),
        job_name: row.try_get("job_name").map_err(backend)?,
        triggered_by: triggered_repr.into_domain(),
        scheduled_at: str_to_dt(&scheduled_at)?,
        started_at: opt_str_to_dt(started_at)?,
        ended_at: opt_str_to_dt(ended_at)?,
        exit_code: exit_code.map(|c| c as i32),
        state: str_to_run_state(&state),
    })
}

#[async_trait]
impl StateRepository for SqliteStore {
    async fn list_specs(&self) -> Result<Vec<ProcessSpec>, RepoError> {
        let rows = sqlx::query("SELECT * FROM process_specs ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(backend)?;
        rows.iter().map(spec_from_row).collect()
    }

    async fn get_spec(&self, name: &str) -> Result<Option<ProcessSpec>, RepoError> {
        let row = sqlx::query("SELECT * FROM process_specs WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        row.as_ref().map(spec_from_row).transpose()
    }

    async fn save_spec(&self, spec: &ProcessSpec) -> Result<(), RepoError> {
        let (mode, unit_name) = match &spec.management_mode {
            ManagementMode::Direct => ("direct", None),
            ManagementMode::SystemRegistered { unit_name } => {
                ("system_registered", Some(unit_name.clone()))
            }
        };
        let lifecycle = match spec.lifecycle {
            LifecycleMode::Tied => "tied",
            LifecycleMode::Detached => "detached",
        };
        let shutdown_signal = match spec.shutdown.signal {
            ShutdownSignal::Term => "term",
            ShutdownSignal::Int => "int",
            ShutdownSignal::Kill => "kill",
        };
        sqlx::query(
            r#"INSERT INTO process_specs
                (name, command, args, cwd, env, mode, unit_name, lifecycle, autostart,
                 restart_enabled, restart_max_retries, restart_backoff_initial_ms,
                 restart_backoff_max_ms, restart_backoff_multiplier, restart_jitter,
                 restart_reset_after_ms, shutdown_signal, shutdown_grace_period_ms,
                 restart_count)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
                       COALESCE((SELECT restart_count FROM process_specs WHERE name = ?), 0))
               ON CONFLICT(name) DO UPDATE SET
                command = excluded.command,
                args = excluded.args,
                cwd = excluded.cwd,
                env = excluded.env,
                mode = excluded.mode,
                unit_name = excluded.unit_name,
                lifecycle = excluded.lifecycle,
                autostart = excluded.autostart,
                restart_enabled = excluded.restart_enabled,
                restart_max_retries = excluded.restart_max_retries,
                restart_backoff_initial_ms = excluded.restart_backoff_initial_ms,
                restart_backoff_max_ms = excluded.restart_backoff_max_ms,
                restart_backoff_multiplier = excluded.restart_backoff_multiplier,
                restart_jitter = excluded.restart_jitter,
                restart_reset_after_ms = excluded.restart_reset_after_ms,
                shutdown_signal = excluded.shutdown_signal,
                shutdown_grace_period_ms = excluded.shutdown_grace_period_ms"#,
        )
        .bind(&spec.name)
        .bind(&spec.command)
        .bind(json_string(&spec.args))
        .bind(spec.cwd.as_ref().map(|p| p.display().to_string()))
        .bind(json_string(&spec.env))
        .bind(mode)
        .bind(unit_name)
        .bind(lifecycle)
        .bind(spec.autostart as i64)
        .bind(spec.restart.enabled as i64)
        .bind(spec.restart.max_retries.map(i64::from))
        .bind(spec.restart.backoff_initial.as_millis().min(i64::MAX as u128) as i64)
        .bind(spec.restart.backoff_max.as_millis().min(i64::MAX as u128) as i64)
        .bind(i64::from(spec.restart.backoff_multiplier))
        .bind(spec.restart.jitter as i64)
        .bind(spec.restart.reset_after.as_millis().min(i64::MAX as u128) as i64)
        .bind(shutdown_signal)
        .bind(spec.shutdown.grace_period.as_millis().min(i64::MAX as u128) as i64)
        .bind(&spec.name)
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn delete_spec(&self, name: &str) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM process_specs WHERE name = ?")
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn get_restart_count(&self, name: &str) -> Result<u32, RepoError> {
        let row = sqlx::query("SELECT restart_count FROM process_specs WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        match row {
            Some(row) => {
                let count: i64 = row.try_get("restart_count").map_err(backend)?;
                Ok(count.max(0) as u32)
            }
            None => Ok(0),
        }
    }

    async fn set_restart_count(&self, name: &str, count: u32) -> Result<(), RepoError> {
        sqlx::query("UPDATE process_specs SET restart_count = ? WHERE name = ?")
            .bind(count as i64)
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn get_runtime_handle(&self, name: &str) -> Result<Option<my_supervisor_core::domain::ChildHandle>, RepoError> {
        let row = sqlx::query(
            "SELECT runtime_process_id, runtime_pid, runtime_started_at FROM process_specs WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let process_id: Option<String> = row.try_get("runtime_process_id").map_err(backend)?;
        let pid: Option<i64> = row.try_get("runtime_pid").map_err(backend)?;
        let started_at: Option<String> = row.try_get("runtime_started_at").map_err(backend)?;
        match (process_id, pid, started_at) {
            (Some(process_id), Some(pid), Some(started_at)) => Ok(Some(
                my_supervisor_core::domain::ChildHandle {
                    process_id: uuid::Uuid::parse_str(&process_id).map_err(backend)?,
                    pid: u32::try_from(pid).map_err(backend)?,
                    started_at: str_to_dt(&started_at)?,
                },
            )),
            _ => Ok(None),
        }
    }

    async fn set_runtime_handle(
        &self,
        name: &str,
        handle: Option<&my_supervisor_core::domain::ChildHandle>,
    ) -> Result<(), RepoError> {
        let (process_id, pid, started_at) = match handle {
            Some(handle) => (
                Some(handle.process_id.to_string()),
                Some(i64::from(handle.pid)),
                Some(dt_to_str(&handle.started_at)),
            ),
            None => (None, None, None),
        };
        sqlx::query(
            "UPDATE process_specs SET runtime_process_id = ?, runtime_pid = ?, runtime_started_at = ? WHERE name = ?",
        )
        .bind(process_id)
        .bind(pid)
        .bind(started_at)
        .bind(name)
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }
}

#[async_trait]
impl JobRepository for SqliteStore {
    async fn list_jobs(&self) -> Result<Vec<Job>, RepoError> {
        let rows = sqlx::query("SELECT * FROM jobs ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(backend)?;
        rows.iter().map(job_from_row).collect()
    }

    async fn get_job(&self, name: &str) -> Result<Option<Job>, RepoError> {
        let row = sqlx::query("SELECT * FROM jobs WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        row.as_ref().map(job_from_row).transpose()
    }

    async fn save_job(&self, job: &Job) -> Result<(), RepoError> {
        let on_overlap = match job.on_overlap {
            my_supervisor_core::domain::OverlapPolicy::Skip => "skip",
            my_supervisor_core::domain::OverlapPolicy::Queue => "queue",
            my_supervisor_core::domain::OverlapPolicy::Parallel => "parallel",
        };
        let on_dep = match job.on_dependency_failure {
            my_supervisor_core::domain::DependencyFailurePolicy::Skip => "skip",
            my_supervisor_core::domain::DependencyFailurePolicy::RunAnyway => "run_anyway",
        };
        sqlx::query(
            r#"INSERT INTO jobs
                (name, id, command, args, cwd, env, trigger, on_overlap, on_dependency_failure,
                 timeout_sec, log_retention_max_runs, log_retention_max_age_days)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(name) DO UPDATE SET
                command = excluded.command,
                args = excluded.args,
                cwd = excluded.cwd,
                env = excluded.env,
                trigger = excluded.trigger,
                on_overlap = excluded.on_overlap,
                on_dependency_failure = excluded.on_dependency_failure,
                timeout_sec = excluded.timeout_sec,
                log_retention_max_runs = excluded.log_retention_max_runs,
                log_retention_max_age_days = excluded.log_retention_max_age_days"#,
        )
        .bind(&job.name)
        .bind(job.id.0.to_string())
        .bind(&job.command)
        .bind(json_string(&job.args))
        .bind(job.cwd.as_ref().map(|p| p.display().to_string()))
        .bind(json_string(&job.env))
        .bind(json_string(&TriggerRepr::from(&job.trigger)))
        .bind(on_overlap)
        .bind(on_dep)
        .bind(job.timeout.map(|d| d.as_secs() as i64))
        .bind(job.log_retention.max_runs.map(i64::from))
        .bind(job.log_retention.max_age_days.map(i64::from))
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn delete_job(&self, name: &str) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM jobs WHERE name = ?")
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn save_run(&self, run: &JobRun) -> Result<(), RepoError> {
        sqlx::query(
            r#"INSERT INTO job_runs
                (run_id, job_name, triggered_by, scheduled_at, started_at, ended_at, exit_code, state)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(run_id) DO UPDATE SET
                started_at = excluded.started_at,
                ended_at = excluded.ended_at,
                exit_code = excluded.exit_code,
                state = excluded.state"#,
        )
        .bind(run.run_id.0.to_string())
        .bind(&run.job_name)
        .bind(json_string(&TriggeredByRepr::from(&run.triggered_by)))
        .bind(dt_to_str(&run.scheduled_at))
        .bind(opt_dt_to_str(&run.started_at))
        .bind(opt_dt_to_str(&run.ended_at))
        .bind(run.exit_code.map(|c| c as i64))
        .bind(run_state_to_str(run.state))
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn list_runs(&self, job_name: &str, limit: usize) -> Result<Vec<JobRun>, RepoError> {
        let rows = sqlx::query(
            "SELECT * FROM job_runs WHERE job_name = ? ORDER BY scheduled_at DESC LIMIT ?",
        )
        .bind(job_name)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.iter().map(run_from_row).collect()
    }

    async fn get_run(
        &self,
        job_name: &str,
        run_id: &JobRunId,
    ) -> Result<Option<JobRun>, RepoError> {
        let row = sqlx::query("SELECT * FROM job_runs WHERE job_name = ? AND run_id = ?")
            .bind(job_name)
            .bind(run_id.0.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        row.as_ref().map(run_from_row).transpose()
    }
}
