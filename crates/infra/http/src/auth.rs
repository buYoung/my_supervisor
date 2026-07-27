//! Bearer authentication shared by every HTTP and WebSocket route.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use subtle::ConstantTimeEq;
use tokio::sync::watch;

use chrono::{DateTime, Utc};
use my_supervisor_shared::api::{
    BackupResultDto, SessionBootstrapDto, TokenRotationDto, UpgradeJournalDto,
};
use my_supervisor_shared::error::ErrorBody;

type RotationHandler = Arc<dyn Fn() -> Result<TokenRotationDto, String> + Send + Sync>;
type BackupHandler = Arc<dyn Fn() -> Result<BackupResultDto, String> + Send + Sync>;
type UpgradeHandler = Arc<dyn Fn() -> Result<UpgradeJournalDto, String> + Send + Sync>;

#[derive(Clone, Default)]
pub struct MaintenanceHandlers {
    pub rotate: Option<RotationHandler>,
    pub backup: Option<BackupHandler>,
    pub upgrade: Option<UpgradeHandler>,
    pub rollback: Option<UpgradeHandler>,
}

#[derive(Clone)]
pub struct AuthVerifier {
    inner: Arc<RwLock<AuthSecret>>,
    generation_tx: watch::Sender<u64>,
    maintenance: Arc<RwLock<MaintenanceHandlers>>,
    sessions: Arc<RwLock<HashMap<String, BrowserSession>>>,
    // Router clones keep the owner lock alive until the final transport drops.
    _lifetime_guard: Option<Arc<dyn Any + Send + Sync>>,
}

struct AuthSecret {
    token: String,
    generation: u64,
}

#[derive(Clone)]
struct BrowserSession {
    generation: u64,
    csrf_token: String,
    issued_at: Instant,
    last_used_at: Instant,
    expires_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct AuthSession {
    pub generation: u64,
    is_browser_session: bool,
}

impl AuthVerifier {
    pub fn new(token: String, generation: u64) -> Self {
        let (generation_tx, _) = watch::channel(generation);
        Self {
            inner: Arc::new(RwLock::new(AuthSecret { token, generation })),
            generation_tx,
            maintenance: Arc::new(RwLock::new(MaintenanceHandlers::default())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            _lifetime_guard: None,
        }
    }

    pub fn retain_for_lifetime<T>(&mut self, guard: Arc<T>)
    where
        T: Any + Send + Sync + 'static,
    {
        self._lifetime_guard = Some(guard);
    }

    pub fn rotate(&self, token: String, generation: u64) {
        let mut secret = self
            .inner
            .write()
            .expect("authentication secret lock poisoned");
        secret.token = token;
        secret.generation = generation;
        let _ = self.generation_tx.send(generation);
        self.sessions
            .write()
            .expect("session lock poisoned")
            .clear();
    }

    pub fn install_maintenance_handlers(&self, handlers: MaintenanceHandlers) {
        *self
            .maintenance
            .write()
            .expect("maintenance handler lock poisoned") = handlers;
    }

    pub fn rotate_token(&self) -> Result<TokenRotationDto, String> {
        self.maintenance
            .read()
            .expect("maintenance handler lock poisoned")
            .rotate
            .as_ref()
            .ok_or_else(|| "token rotation is unavailable for this host".to_string())?()
    }

    pub fn backup(&self) -> Result<BackupResultDto, String> {
        self.maintenance
            .read()
            .expect("maintenance handler lock poisoned")
            .backup
            .as_ref()
            .ok_or_else(|| "backup is unavailable for this host".to_string())?()
    }

    pub fn upgrade(&self) -> Result<UpgradeJournalDto, String> {
        self.maintenance
            .read()
            .expect("maintenance handler lock poisoned")
            .upgrade
            .as_ref()
            .ok_or_else(|| "upgrade is unavailable for this host".to_string())?()
    }

    pub fn rollback(&self) -> Result<UpgradeJournalDto, String> {
        self.maintenance
            .read()
            .expect("maintenance handler lock poisoned")
            .rollback
            .as_ref()
            .ok_or_else(|| "rollback is unavailable for this host".to_string())?()
    }

    fn authenticate(&self, authorization: Option<&axum::http::HeaderValue>) -> Option<AuthSession> {
        let value = authorization?.to_str().ok()?;
        let token = value.strip_prefix("Bearer ")?;
        let secret = self.inner.read().ok()?;
        (token.as_bytes().ct_eq(secret.token.as_bytes()).unwrap_u8() == 1).then_some(AuthSession {
            generation: secret.generation,
            is_browser_session: false,
        })
    }

    /// Exchanges an already-verified native bearer for an opaque, server-side
    /// browser session. The bearer is neither stored nor returned.
    pub fn bootstrap_session(&self, generation: u64) -> (String, SessionBootstrapDto) {
        let now = Instant::now();
        let session_id = uuid::Uuid::new_v4().simple().to_string();
        let csrf_token = uuid::Uuid::new_v4().simple().to_string();
        let expires_at = Utc::now() + chrono::Duration::minutes(10);
        self.sessions
            .write()
            .expect("session lock poisoned")
            .insert(
                session_id.clone(),
                BrowserSession {
                    generation,
                    csrf_token: csrf_token.clone(),
                    issued_at: now,
                    last_used_at: now,
                    expires_at,
                },
            );
        (
            session_id,
            SessionBootstrapDto {
                csrf_token,
                expires_at,
            },
        )
    }

    pub fn logout_session(&self, cookie: Option<&HeaderValue>) {
        if let Some(id) = session_id_from_cookie(cookie) {
            self.sessions
                .write()
                .expect("session lock poisoned")
                .remove(id);
        }
    }

    fn authenticate_session(&self, request: &Request<Body>) -> Option<AuthSession> {
        if !is_strict_loopback_origin(request.headers().get(header::ORIGIN)) {
            return None;
        }
        let id = session_id_from_cookie(request.headers().get(header::COOKIE))?;
        let secret = self.inner.read().ok()?;
        let mut sessions = self.sessions.write().ok()?;
        let session = sessions.get_mut(id)?;
        if session.generation != secret.generation
            || session.issued_at.elapsed() > Duration::from_secs(15 * 60)
            || session.last_used_at.elapsed() > Duration::from_secs(10 * 60)
            || Utc::now() > session.expires_at
        {
            sessions.remove(id);
            return None;
        }
        let is_mutation = !matches!(
            *request.method(),
            axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
        );
        if is_mutation
            && request
                .headers()
                .get("x-csrf-token")
                .and_then(|v| v.to_str().ok())
                != Some(session.csrf_token.as_str())
        {
            return None;
        }
        session.last_used_at = Instant::now();
        Some(AuthSession {
            generation: session.generation,
            is_browser_session: true,
        })
    }

    pub fn generation_receiver(&self) -> watch::Receiver<u64> {
        self.generation_tx.subscribe()
    }
}

impl AuthSession {
    pub fn is_browser_session(&self) -> bool {
        self.is_browser_session
    }
}

fn session_id_from_cookie(cookie: Option<&HeaderValue>) -> Option<&str> {
    cookie?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|item| item.strip_prefix("msv_session="))
}

fn is_strict_loopback_origin(origin: Option<&HeaderValue>) -> bool {
    let Some(origin) = origin else {
        return false;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    origin == "http://localhost"
        || origin == "http://127.0.0.1"
        || origin.starts_with("http://localhost:")
        || origin.starts_with("http://127.0.0.1:")
}

pub async fn require_bearer(
    verifier: axum::extract::State<AuthVerifier>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let session = verifier
        .authenticate(request.headers().get(header::AUTHORIZATION))
        .or_else(|| verifier.authenticate_session(&request));
    let Some(session) = session else {
        return unauthorized();
    };
    request.extensions_mut().insert(session);
    request.extensions_mut().insert(verifier.0.clone());
    next.run(request).await
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorBody::new(
            "unauthorized",
            "valid bearer authorization is required",
        )),
    )
        .into_response()
}
