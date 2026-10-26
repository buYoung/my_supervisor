use super::{backend, SqliteStore};
use chrono::Utc;
use my_supervisor_core::{domain::WorkspaceDocument, ports::RepoError};
use sqlx::Row;

fn document(row: sqlx::sqlite::SqliteRow) -> Result<WorkspaceDocument, RepoError> {
    Ok(WorkspaceDocument {
        revision: row.try_get::<i64, _>("revision").map_err(backend)? as u64,
        saved_at: row
            .try_get::<String, _>("saved_at")
            .map_err(backend)?
            .parse()
            .map_err(backend)?,
        value: row.try_get("value").map_err(backend)?,
    })
}
impl SqliteStore {
    pub(super) async fn delete_job_workspace(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        name: &str,
    ) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM workspace_state WHERE key IN (SELECT 'workflow-run/' || run_id FROM job_runs WHERE job_name=? UNION SELECT 'workflow-override/' || run_id FROM job_runs WHERE job_name=?)").bind(name).bind(name).execute(&mut **transaction).await.map_err(backend)?;
        sqlx::query("DELETE FROM workspace_state WHERE EXISTS (SELECT 1 FROM jobs WHERE name=? AND workspace_state.key LIKE 'circuit/' || jobs.id || '/%')").bind(name).execute(&mut **transaction).await.map_err(backend)?;
        sqlx::query("DELETE FROM workspace_documents WHERE key IN (SELECT 'workflow/' || id FROM jobs WHERE name=?)").bind(name).execute(&mut **transaction).await.map_err(backend)?;
        Ok(())
    }
    pub(super) async fn read_workspace_document(
        &self,
        key: &str,
        revision: Option<u64>,
    ) -> Result<Option<WorkspaceDocument>, RepoError> {
        let row = sqlx::query("SELECT revision,saved_at,value FROM workspace_documents WHERE key=? AND (? IS NULL OR revision=?) ORDER BY revision DESC LIMIT 1")
            .bind(key).bind(revision.map(|v|v as i64)).bind(revision.map(|v|v as i64)).fetch_optional(&self.pool).await.map_err(backend)?;
        row.map(document).transpose()
    }
    pub(super) async fn read_workspace_versions(
        &self,
        key: &str,
    ) -> Result<Vec<WorkspaceDocument>, RepoError> {
        sqlx::query("SELECT revision,saved_at,value FROM workspace_documents WHERE key=? ORDER BY revision DESC LIMIT 100")
            .bind(key).fetch_all(&self.pool).await.map_err(backend)?.into_iter().map(document).collect()
    }
    pub(super) async fn write_workspace_document(
        &self,
        key: &str,
        value: &str,
        expected: u64,
    ) -> Result<WorkspaceDocument, RepoError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let current: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(revision),0) FROM workspace_documents WHERE key=?",
        )
        .bind(key)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        if current as u64 != expected {
            return Err(RepoError::Conflict(
                "workspace revision changed; reload before saving".into(),
            ));
        }
        let saved_at = Utc::now();
        let revision = expected + 1;
        sqlx::query(
            "INSERT INTO workspace_documents(key,revision,saved_at,value) VALUES (?,?,?,?)",
        )
        .bind(key)
        .bind(revision as i64)
        .bind(saved_at.to_rfc3339())
        .bind(value)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        sqlx::query("DELETE FROM workspace_documents WHERE key=? AND revision <= ?")
            .bind(key)
            .bind(revision.saturating_sub(100) as i64)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(WorkspaceDocument {
            revision,
            saved_at,
            value: value.into(),
        })
    }
}
