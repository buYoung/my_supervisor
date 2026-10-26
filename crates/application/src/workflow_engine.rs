use crate::{
    workflow::{number, text, Node, Workflow},
    OperationsFacade,
};
use my_supervisor_core::{
    domain::{Job, JobRunId, LogLine, LogStream},
    ports::{RunExecutionControl, TransientCompletion, TransientOutcome},
};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{watch, Mutex};

type Task<T> = Pin<Box<dyn Future<Output = T> + Send>>;
fn bounded_text(value: &str, limit: usize) -> &str {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}
fn capture_line(output: &mut String, line: &str) {
    let remaining = 65_536usize.saturating_sub(output.len());
    if remaining == 0 {
        return;
    }
    output.push_str(bounded_text(line, remaining));
    if output.len() < 65_536 {
        output.push('\n');
    }
}
#[derive(Clone, Default)]
struct ResultData {
    state: String,
    outcome: String,
    code: i32,
    stdout: String,
    stderr: String,
    port: String,
}
impl ResultData {
    fn ok() -> Self {
        Self {
            state: "succeeded".into(),
            outcome: "succeeded".into(),
            ..Default::default()
        }
    }
    fn failed(message: String) -> Self {
        Self {
            state: "failed".into(),
            outcome: "failed".into(),
            code: 1,
            stderr: message,
            ..Default::default()
        }
    }
    fn cancelled() -> Self {
        Self {
            state: "cancelled".into(),
            outcome: "cancelled".into(),
            code: 130,
            ..Default::default()
        }
    }
}
struct PathScope {
    single: bool,
    prefix: String,
    chain: HashSet<String>,
}

struct Engine {
    facade: Arc<OperationsFacade>,
    job: Job,
    run_id: JobRunId,
    state: Mutex<Value>,
    persist_lock: Mutex<()>,
    control: Arc<dyn RunExecutionControl>,
    has_unreaped: AtomicBool,
    pending_cleanup: Mutex<Vec<my_supervisor_core::ports::CleanupTicket>>,
}

pub(crate) async fn execute(
    facade: Arc<OperationsFacade>,
    job: &Job,
    id: JobRunId,
    control: Arc<dyn RunExecutionControl>,
    flow: Workflow,
    revision: u64,
    only: Option<String>,
) -> Result<TransientCompletion, my_supervisor_core::ports::RunnerError> {
    crate::workflow::validate(&flow)
        .map_err(|e| my_supervisor_core::ports::RunnerError::Backend(e.to_string()))?;
    let started = facade.workspace_deps().clock.now();
    let steps=flow.nodes.iter().map(|n|(n.id.clone(),json!({"id":n.id,"label":text(&n.data,"label"),"kind":text(&n.data,"kind"),"state":"pending","logs":[]}))).collect::<serde_json::Map<_,_>>();
    let engine = Arc::new(Engine {
        facade,
        job: job.clone(),
        run_id: id,
        state: Mutex::new(
            json!({"id":id.0,"jobId":job.name,"version":revision,"workflow":flow,"state":"running","startedAt":started,"steps":steps,"edges":[],"activeNodeIds":[]}),
        ),
        persist_lock: Mutex::new(()),
        control: control.clone(),
        has_unreaped: AtomicBool::new(false),
        pending_cleanup: Mutex::new(Vec::new()),
    });
    if !engine.persist().await {
        return Err(my_supervisor_core::ports::RunnerError::Backend(
            "cannot persist workflow start".into(),
        ));
    }
    let (cancel, receiver) = watch::channel(false);
    let mut parent = control.cancellation();
    let cancelled = cancel.clone();
    let timeout = job.timeout.unwrap_or(Duration::from_secs(86400));
    let timed_out = Arc::new(AtomicBool::new(false));
    let timed = timed_out.clone();
    let monitor = tokio::spawn(async move {
        if *parent.borrow() {
            cancelled.send_replace(true);
            return;
        }
        tokio::select! { _=parent.changed()=>{cancelled.send_replace(true);}, _=tokio::time::sleep(timeout)=>{timed.store(true,Ordering::SeqCst);cancelled.send_replace(true);} }
    });
    let start = only.clone().unwrap_or_else(|| {
        flow.nodes
            .iter()
            .find(|n| text(&n.data, "kind") == "trigger")
            .unwrap()
            .id
            .clone()
    });
    let result = engine
        .clone()
        .path(
            Arc::new(flow),
            start,
            None,
            ResultData::ok(),
            receiver,
            PathScope {
                single: only.is_some(),
                prefix: String::new(),
                chain: HashSet::from([job.name.clone()]),
            },
        )
        .await;
    monitor.abort();
    let ended = engine.facade.workspace_deps().clock.now();
    let state = if timed_out.load(Ordering::SeqCst) {
        "timed_out"
    } else {
        &result.state
    };
    {
        let mut record = engine.state.lock().await;
        record["state"] = json!(state);
        record["finishedAt"] = json!(ended);
        record["durationMs"] = json!((ended - started).num_milliseconds());
        record["exitCode"] = json!(result.code);
        record["summary"] = json!(if result.stderr.is_empty() {
            result.outcome.clone()
        } else {
            result.stderr.clone()
        });
        for step in record["steps"].as_object_mut().unwrap().values_mut() {
            if step["state"] == "pending" {
                step["state"] = json!("skipped");
            }
        }
    }
    let has_persisted = engine.persist().await;
    if engine.has_unreaped.load(Ordering::SeqCst) {
        {
            let mut record = engine.state.lock().await;
            record["state"] = json!("running");
            record["summary"] = json!("workflow child cleanup is pending");
        }
        engine.persist().await;
        for ticket in engine.pending_cleanup.lock().await.drain(..) {
            loop {
                match engine
                    .facade
                    .workspace_deps()
                    .job_repo
                    .enqueue_transient_cleanup(&ticket)
                    .await
                {
                    Ok(()) => break,
                    Err(error) => {
                        tracing::error!(%error,"persist workflow child cleanup");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }
        return Err(my_supervisor_core::ports::RunnerError::Unreaped(
            "workflow child cleanup is pending".into(),
        ));
    }
    if !has_persisted {
        return Err(my_supervisor_core::ports::RunnerError::Backend(
            "cannot persist workflow result".into(),
        ));
    }
    let outcome = TransientOutcome {
        started_at: started,
        ended_at: ended,
        exit_code: Some(result.code),
    };
    Ok(if timed_out.load(Ordering::SeqCst) {
        TransientCompletion::TimedOut(outcome)
    } else if state == "cancelled" {
        TransientCompletion::Cancelled(outcome)
    } else {
        TransientCompletion::Exited(outcome)
    })
}

impl Engine {
    async fn persist(&self) -> bool {
        let _serial = self.persist_lock.lock().await;
        let serialized = self.state.lock().await.to_string();
        for attempt in 0..3 {
            match self
                .facade
                .workspace_deps()
                .state_repo
                .save_workspace_state(&format!("workflow-run/{}", self.run_id.0), &serialized)
                .await
            {
                Ok(()) => return true,
                Err(error) => {
                    tracing::error!(%error,attempt,"persist workflow execution");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
        false
    }
    async fn log(&self, id: &str, stream: LogStream, message: &str) {
        let now = self.facade.workspace_deps().clock.now();
        let original = serde_json::from_str::<Value>(message).ok();
        let level = original
            .as_ref()
            .and_then(|v| v.get("level"))
            .cloned()
            .unwrap_or(json!("unknown"));
        let line = json!({"level":level,"step_id":id,"message":message});
        let _ = self
            .facade
            .workspace_deps()
            .log_sink
            .append_run(self.run_id, LogLine::now(stream, line.to_string()))
            .await;
        let mut record = self.state.lock().await;
        if let Some(step) = record["steps"].get_mut(id) {
            let logs = step["logs"].as_array_mut().unwrap();
            if logs.len() >= 200 {
                logs.remove(0);
            }
            logs.push(json!({"time":now,"stream":crate::log_explorer::stream_name(stream),"message":bounded_text(message,4096)}));
            if logs.len() % 16 != 0 {
                return;
            }
        }
        drop(record);
        self.persist().await;
    }
    async fn begin(&self, id: &str, node: &Node, attempt: u64) {
        let mut record = self.state.lock().await;
        if record["steps"].get(id).is_none() {
            record["steps"][id] = json!({"id":id,"label":text(&node.data,"label"),"kind":text(&node.data,"kind"),"logs":[]});
        }
        let step = &mut record["steps"][id];
        step["state"] = json!("running");
        step["attempt"] = json!(attempt);
        step["startedAt"] = json!(self.facade.workspace_deps().clock.now());
        drop(record);
        self.persist().await;
    }
    async fn finish(&self, id: &str, result: &ResultData) {
        let mut record = self.state.lock().await;
        let step = &mut record["steps"][id];
        step["state"] = json!(result.state);
        step["outcome"] = json!(result.outcome);
        step["finishedAt"] = json!(self.facade.workspace_deps().clock.now());
        step["exitCode"] = json!(result.code);
        step["stdout"] = json!(result.stdout);
        step["stderr"] = json!(result.stderr);
        step["selectedPort"] = json!(result.port);
        step["summary"] = json!(if result.stderr.is_empty() {
            result.outcome.clone()
        } else {
            result.stderr.clone()
        });
        drop(record);
        self.persist().await;
    }
    fn path(
        self: Arc<Self>,
        flow: Arc<Workflow>,
        mut id: String,
        stop: Option<String>,
        mut last: ResultData,
        mut cancel: watch::Receiver<bool>,
        scope: PathScope,
    ) -> Task<ResultData> {
        let PathScope {
            single,
            prefix,
            chain,
        } = scope;
        Box::pin(async move {
            for _ in 0..201 {
                if stop.as_ref() == Some(&id) {
                    return last;
                }
                if *cancel.borrow() {
                    return ResultData::cancelled();
                }
                let Some(node) = flow.nodes.iter().find(|n| n.id == id).cloned() else {
                    return ResultData::failed("step not found".into());
                };
                let full_id = format!("{prefix}{id}");
                let mut result = self
                    .clone()
                    .with_policy(
                        node.clone(),
                        full_id.clone(),
                        last.clone(),
                        cancel.clone(),
                        chain.clone(),
                    )
                    .await;
                let kind = text(&node.data, "kind");
                if kind == "condition" {
                    result.port = if result.code == 0 { "true" } else { "false" }.into();
                    result.state = "succeeded".into();
                    result.outcome = "succeeded".into();
                    result.code = 0;
                } else {
                    result.port = select_port(&node.data, &result);
                }
                self.finish(&full_id, &result).await;
                if self.has_unreaped.load(Ordering::SeqCst) {
                    return result;
                }
                if single || kind == "finish" {
                    return result;
                }
                if kind == "parallel" {
                    let branches: Vec<_> = flow
                        .edges
                        .iter()
                        .filter(|e| e.source == id)
                        .cloned()
                        .collect();
                    let join = find_join(
                        &flow,
                        &branches
                            .iter()
                            .map(|e| e.target.clone())
                            .collect::<Vec<_>>(),
                    );
                    let mode = join
                        .as_ref()
                        .and_then(|id| flow.nodes.iter().find(|n| &n.id == id))
                        .map(|n| text(&n.data, "joinMode"))
                        .unwrap_or("all");
                    let mut tasks = tokio::task::JoinSet::new();
                    let mut cancellations = Vec::new();
                    for branch in branches {
                        let (tx, rx) = watch::channel(*cancel.borrow());
                        cancellations.push(tx);
                        let path = self.clone().path(
                            flow.clone(),
                            branch.target,
                            join.clone(),
                            result.clone(),
                            rx,
                            PathScope {
                                single: false,
                                prefix: prefix.clone(),
                                chain: chain.clone(),
                            },
                        );
                        let branch_id = branch.id.clone();
                        {
                            let mut record = self.state.lock().await;
                            record["edges"]
                                .as_array_mut()
                                .unwrap()
                                .push(json!(branch_id));
                        }
                        tasks.spawn(async move { (branch_id, path.await) });
                    }
                    let mut winner = None;
                    let mut failure = None;
                    let mut cancel_seen = false;
                    while !tasks.is_empty() {
                        tokio::select! {
                            _=cancel.changed(),if !cancel_seen=>{cancel_seen=true;for tx in &cancellations {tx.send_replace(true);}},
                            item=tasks.join_next()=>{match item {Some(Ok((branch_id,value)))=>{if value.state!="succeeded"&&(failure.is_none()||value.state!="cancelled") {failure=Some(value.clone());}
                                if winner.is_none()&&(mode=="race"||mode=="first_success"&&value.state=="succeeded") {winner=Some(value);{let mut record=self.state.lock().await;record["steps"][&full_id]["winner"]=json!(branch_id);}for tx in &cancellations{tx.send_replace(true);}}
                            },Some(Err(error))=>{failure=Some(ResultData::failed(format!("parallel branch failed: {error}")));for tx in &cancellations{tx.send_replace(true);}},None=>{}}}
                        }
                    }
                    if *cancel.borrow() {
                        return ResultData::cancelled();
                    }
                    result = winner.unwrap_or_else(|| failure.unwrap_or_else(ResultData::ok));
                    if mode == "all"
                        && result.state != "succeeded"
                        && join
                            .as_ref()
                            .and_then(|id| flow.nodes.iter().find(|n| &n.id == id))
                            .is_none_or(|n| text(&n.data, "failurePolicy") != "collect")
                    {
                        return result;
                    }
                    if let Some(next) = join {
                        id = next;
                        last = result;
                        continue;
                    } else {
                        return result;
                    }
                }
                let edge = flow
                    .edges
                    .iter()
                    .find(|e| e.source == id && e.source_handle == result.port);
                if result.state != "succeeded"
                    && (result.port == "out" || edge.is_none())
                    && text(&node.data, "onError") != "continue"
                {
                    let has_condition = edge
                        .and_then(|e| flow.nodes.iter().find(|n| n.id == e.target))
                        .is_some_and(|n| text(&n.data, "kind") == "condition");
                    if !has_condition {
                        return result;
                    }
                }
                let Some(edge) = edge else {
                    return result;
                };
                {
                    let mut record = self.state.lock().await;
                    record["edges"].as_array_mut().unwrap().push(json!(edge.id));
                }
                id = edge.target.clone();
                last = result;
            }
            ResultData::failed("workflow step limit exceeded".into())
        })
    }
    fn with_policy(
        self: Arc<Self>,
        node: Node,
        id: String,
        last: ResultData,
        cancel: watch::Receiver<bool>,
        chain: HashSet<String>,
    ) -> Task<ResultData> {
        Box::pin(async move {
            let data = &node.data;
            let circuit = &data["circuitBreaker"];
            let circuit_key = format!("circuit/{}/{}", self.job.id.0, id);
            let circuit_lock = self
                .facade
                .workflow_lock(&format!("circuit/{}/{}", self.job.id.0, id));
            let mut wait_cancel = cancel.clone();
            let _guard = if circuit["enabled"] == true {
                Some(
                    tokio::select! {guard=circuit_lock.lock()=>guard,_=wait_cancel.changed()=>return ResultData::cancelled()},
                )
            } else {
                None
            };
            let mut failures = 0u64;
            let mut recovery_successes = 0u64;
            let mut is_probe = false;
            if circuit["enabled"] == true {
                if let Ok(Some(saved)) = self
                    .facade
                    .workspace_deps()
                    .state_repo
                    .workspace_state(&circuit_key)
                    .await
                {
                    if let Ok(saved) = serde_json::from_str::<Value>(&saved) {
                        failures = saved["failures"].as_u64().unwrap_or(0);
                        let open_until = saved["openUntil"].as_i64().unwrap_or(0);
                        is_probe = open_until > 0;
                        recovery_successes = saved["recoverySuccesses"].as_u64().unwrap_or(0);
                        if open_until > self.facade.workspace_deps().clock.now().timestamp_millis()
                        {
                            self.begin(&id, &node, 0).await;
                            let mut result = ResultData::failed("circuit breaker is open".into());
                            result.outcome = "circuit_open".into();
                            return result;
                        }
                    }
                }
            }
            let attempts = number(data, "maxAttempts", 1.0).clamp(1.0, 10.0) as u64;
            let mut result = ResultData::ok();
            for attempt in 1..=attempts {
                if *cancel.borrow() {
                    return ResultData::cancelled();
                }
                self.begin(&id, &node, attempt).await;
                let (step_cancel, step_receiver) = watch::channel(*cancel.borrow());
                let mut parent = cancel.clone();
                let expired = Arc::new(AtomicBool::new(false));
                let timer_expired = expired.clone();
                let seconds = number(data, "timeoutSeconds", 60.0);
                let timer = tokio::spawn(async move {
                    tokio::select! {_=parent.changed()=>{step_cancel.send_replace(true);},_=tokio::time::sleep(Duration::from_secs_f64(seconds))=>{timer_expired.store(true,Ordering::SeqCst);step_cancel.send_replace(true);}}
                });
                result = self
                    .clone()
                    .perform(
                        node.clone(),
                        id.clone(),
                        last.clone(),
                        step_receiver,
                        chain.clone(),
                    )
                    .await;
                timer.abort();
                if *cancel.borrow() {
                    result = ResultData::cancelled();
                } else if expired.load(Ordering::SeqCst) {
                    result.state = "timed_out".into();
                    result.outcome = "timed_out".into();
                    result.code = 124;
                }
                if result.state == "succeeded"
                    || result.state == "cancelled"
                    || self.has_unreaped.load(Ordering::SeqCst)
                    || attempt == attempts
                {
                    break;
                }
                let retry_on = data.get("retryOn");
                if retry_on.is_some_and(|value| {
                    !value.as_array().is_some_and(|items| {
                        items.iter().any(|v| v == &result.outcome || v == "all")
                    })
                }) {
                    break;
                }
                let base = number(data, "retryDelaySeconds", 1.0);
                let mut delay = match text(data, "retryStrategy") {
                    "linear" => {
                        base + (attempt - 1) as f64 * number(data, "retryIncrementSeconds", 1.0)
                    }
                    "exponential" => {
                        base * number(data, "retryMultiplier", 2.0).powi((attempt - 1) as i32)
                    }
                    _ => base,
                };
                delay = delay
                    .min(number(data, "retryMaxDelaySeconds", 60.0))
                    .max(0.01);
                if data["retryJitter"] == true {
                    delay *= 0.8 + (uuid::Uuid::new_v4().as_u128() % 400) as f64 / 1000.0;
                }
                self.log(
                    &id,
                    LogStream::System,
                    &format!("retry {}/{} in {:.2}s", attempt + 1, attempts, delay),
                )
                .await;
                let mut receiver = cancel.clone();
                tokio::select! {_=tokio::time::sleep(Duration::from_secs_f64(delay))=>{},_=receiver.changed()=>return ResultData::cancelled()}
            }
            if circuit["enabled"] == true && result.state != "cancelled" {
                failures = if result.state == "succeeded" {
                    0
                } else {
                    failures + 1
                };
                recovery_successes = if result.state == "succeeded" {
                    recovery_successes + 1
                } else {
                    0
                };
                let open_until = if result.state != "succeeded"
                    && (is_probe || failures >= number(circuit, "failureThreshold", 3.0) as u64)
                {
                    self.facade.workspace_deps().clock.now().timestamp_millis()
                        + (number(circuit, "openSeconds", 30.0) * 1000.0) as i64
                } else if is_probe
                    && recovery_successes < number(circuit, "recoverySuccesses", 1.0) as u64
                {
                    1
                } else {
                    0
                };
                if let Err(error)=self.facade.workspace_deps().state_repo.save_workspace_state(&circuit_key,&json!({"failures":failures,"openUntil":open_until,"recoverySuccesses":recovery_successes}).to_string()).await {return ResultData::failed(format!("cannot persist circuit state: {error}"));}
            }
            result
        })
    }
    fn perform(
        self: Arc<Self>,
        node: Node,
        id: String,
        last: ResultData,
        mut cancel: watch::Receiver<bool>,
        mut chain: HashSet<String>,
    ) -> Task<ResultData> {
        Box::pin(async move {
            let data = &node.data;
            let kind = text(data, "kind");
            self.log(
                &id,
                LogStream::System,
                &format!("step: {}", text(data, "label")),
            )
            .await;
            match kind {
                "trigger" | "parallel" => ResultData::ok(),
                "join" => {
                    if text(data, "failurePolicy") == "collect" {
                        let mut result = last;
                        result.state = "succeeded".into();
                        result.outcome = "succeeded".into();
                        result.code = 0;
                        result
                    } else {
                        last
                    }
                }
                "finish" => {
                    if text(data, "result") == "failed" {
                        ResultData::failed(text(data, "message").into())
                    } else {
                        ResultData::ok()
                    }
                }
                "wait" => {
                    tokio::select! {_=tokio::time::sleep(Duration::from_secs_f64(number(data,"seconds",1.0).clamp(0.0,86400.0)))=>ResultData::ok(),_=cancel.changed()=>ResultData::cancelled()}
                }
                "condition" => {
                    let actual = match text(data, "source") {
                        "stdout" => json!(last.stdout),
                        "stderr" => json!(last.stderr),
                        "outcome" => json!(last.outcome),
                        _ => json!(last.code),
                    };
                    let yes = if text(data, "source") == "file_exists" {
                        std::path::Path::new(text(data, "path")).exists()
                    } else {
                        compare(&actual, text(data, "operator"), &data["value"])
                    };
                    let mut result = ResultData::ok();
                    result.code = if yes { 0 } else { 1 };
                    result
                }
                "process" => {
                    let name = text(data, "processName");
                    let result = match text(data, "operation") {
                        "start" => self.facade.start_process(name).await.map(|_| ()),
                        "stop" => self.facade.stop_process(name, false).await.map(|_| ()),
                        _ => self.facade.restart_process(name).await.map(|_| ()),
                    };
                    result.map_or_else(|e| ResultData::failed(e.to_string()), |_| ResultData::ok())
                }
                "health" => {
                    use my_supervisor_core::domain::{CheckKind, CheckPolicy};
                    if text(data, "checkType") == "process" {
                        return match self.facade.get_process(text(data, "processName")).await {
                            Ok(status)
                                if status.state
                                    == my_supervisor_core::domain::ProcessState::Running =>
                            {
                                ResultData::ok()
                            }
                            _ => ResultData::failed("process is not running".into()),
                        };
                    }
                    let kind = if text(data, "checkType") == "tcp" {
                        CheckKind::Tcp {
                            host: text(data, "host").into(),
                            port: number(data, "port", 80.0) as u16,
                        }
                    } else {
                        CheckKind::Http {
                            url: text(data, "url").into(),
                            expected_status: number(data, "expectedStatus", 200.0) as u16,
                        }
                    };
                    let policy = CheckPolicy {
                        kind,
                        interval: Duration::from_secs(1),
                        timeout: Duration::from_secs_f64(
                            number(data, "timeoutSeconds", 10.0).clamp(1.0, 86400.0),
                        ),
                        consecutive_successes: 1,
                        consecutive_failures: 1,
                    };
                    match self
                        .facade
                        .workspace_deps()
                        .lifecycle
                        .run_check(&policy, &mut cancel)
                        .await
                    {
                        Ok(my_supervisor_core::ports::CheckOutcome::Succeeded { .. }) => {
                            ResultData::ok()
                        }
                        other => ResultData::failed(format!("health check: {other:?}")),
                    }
                }
                "job" => {
                    let target = text(data, "jobId");
                    if !chain.insert(target.into()) || chain.len() > 16 {
                        return ResultData::failed("recursive job call".into());
                    }
                    match self.facade.get_job(target).await {
                        Ok(view) => {
                            let document = self
                                .facade
                                .workspace_deps()
                                .state_repo
                                .workspace_document(&crate::workflow::key(&view.job), None)
                                .await
                                .ok()
                                .flatten();
                            let flow = document
                                .and_then(|d| serde_json::from_str::<Workflow>(&d.value).ok())
                                .unwrap_or_else(|| crate::workflow::default_workflow(&view.job));
                            let start = flow
                                .nodes
                                .iter()
                                .find(|n| text(&n.data, "kind") == "trigger")
                                .map(|n| n.id.clone())
                                .unwrap_or_default();
                            self.clone()
                                .path(
                                    Arc::new(flow),
                                    start,
                                    None,
                                    ResultData::ok(),
                                    cancel,
                                    PathScope {
                                        single: false,
                                        prefix: format!("{id}/"),
                                        chain,
                                    },
                                )
                                .await
                        }
                        Err(e) => ResultData::failed(e.to_string()),
                    }
                }
                "command" => self.command(&id, data, cancel).await,
                _ => ResultData::failed("unsupported step".into()),
            }
        })
    }
    async fn command(
        self: Arc<Self>,
        id: &str,
        data: &Value,
        mut cancel: watch::Receiver<bool>,
    ) -> ResultData {
        let mut job = self.job.clone();
        if !text(data, "program").is_empty() {
            job.command = text(data, "program").into();
            job.args = data["args"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
        } else {
            job.command = "/bin/sh".into();
            job.args = vec!["-c".into(), text(data, "command").into()];
        }
        if data.get("cwd").is_some() {
            job.cwd = if text(data, "cwd").is_empty() {
                None
            } else {
                Some(text(data, "cwd").into())
            };
        }
        if let Some(env) = data["environment"].as_object() {
            job.env = env
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_owned()))
                })
                .collect();
        }
        let spec = crate::runner::job_to_spec(&job);
        let child_id = JobRunId::new();
        let mut logs = self
            .facade
            .workspace_deps()
            .log_sink
            .subscribe_run(child_id);
        let child = match self
            .facade
            .workspace_deps()
            .lifecycle
            .start_transient(&spec, child_id)
            .await
        {
            Ok(child) => child,
            Err(e) => return ResultData::failed(e.to_string()),
        };
        self.control.publish_child(child.clone()).await;
        let copied = self.clone();
        let step_id = id.to_string();
        let (done_tx, mut done_rx) = tokio::sync::oneshot::channel();
        let captured = tokio::spawn(async move {
            let mut stdout = String::new();
            let mut stderr = String::new();
            loop {
                let line = tokio::select! {line=logs.recv()=>line.ok(),_= &mut done_rx=>{while let Ok(line)=logs.try_recv(){copied.log(&step_id,line.stream,&line.line).await;let output=if line.stream==LogStream::Stderr{&mut stderr}else{&mut stdout};capture_line(output,&line.line);}break;}};
                if let Some(line) = line {
                    copied.log(&step_id, line.stream, &line.line).await;
                    let output = if line.stream == LogStream::Stderr {
                        &mut stderr
                    } else {
                        &mut stdout
                    };
                    capture_line(output, &line.line);
                } else {
                    break;
                }
            }
            (stdout, stderr)
        });
        let completion = self
            .facade
            .workspace_deps()
            .lifecycle
            .complete_transient(
                &child,
                Some(Duration::from_secs_f64(
                    number(data, "timeoutSeconds", 60.0).clamp(1.0, 86400.0),
                )),
                &mut cancel,
            )
            .await;
        let _ = done_tx.send(());
        let (stdout, stderr) = captured.await.unwrap_or_default();
        let mut result = match completion {
            Ok(TransientCompletion::Exited(outcome)) => {
                let code = outcome.exit_code.unwrap_or(1);
                ResultData {
                    state: if code == 0 { "succeeded" } else { "failed" }.into(),
                    outcome: if code == 0 { "succeeded" } else { "failed" }.into(),
                    code,
                    ..Default::default()
                }
            }
            Ok(TransientCompletion::Cancelled(_)) => ResultData::cancelled(),
            Ok(TransientCompletion::TimedOut(_)) => ResultData {
                state: "timed_out".into(),
                outcome: "timed_out".into(),
                code: 124,
                ..Default::default()
            },
            Ok(TransientCompletion::CleanupPending {
                cause,
                stage,
                outcome,
                ..
            }) => {
                self.has_unreaped.store(true, Ordering::SeqCst);
                let ticket = my_supervisor_core::ports::CleanupTicket {
                    cleanup_id: uuid::Uuid::new_v4(),
                    job_id: self.job.id,
                    job_name: self.job.name.clone(),
                    run_id: self.run_id,
                    child,
                    stage,
                    attempts: 0,
                    last_error: Some(cause.clone()),
                    intended_terminal_state: my_supervisor_core::domain::JobRunState::Failed,
                    outcome,
                };
                self.pending_cleanup.lock().await.push(ticket);
                ResultData::failed(cause)
            }
            Err(e) => ResultData::failed(e.to_string()),
        };
        result.stdout = stdout;
        if !stderr.is_empty() {
            result.stderr = stderr;
        }
        let _ = self
            .facade
            .workspace_deps()
            .log_sink
            .seal_run(child_id)
            .await;
        result
    }
}
fn compare(actual: &Value, operator: &str, expected: &Value) -> bool {
    let a = actual
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| actual.to_string());
    let b = expected
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| expected.to_string());
    match operator {
        "not_equals" => a != b,
        "gte" => a.parse::<f64>().unwrap_or(f64::NAN) >= b.parse::<f64>().unwrap_or(f64::NAN),
        "lte" => a.parse::<f64>().unwrap_or(f64::NAN) <= b.parse::<f64>().unwrap_or(f64::NAN),
        "includes" | "contains" => a.contains(&b),
        "not_includes" => !a.contains(&b),
        _ => a == b,
    }
}
fn select_port(data: &Value, result: &ResultData) -> String {
    if let Some(routes) = data["resultRoutes"].as_array() {
        for route in routes {
            let actual = match text(route, "source") {
                "stdout" => json!(result.stdout),
                "stderr" => json!(result.stderr),
                "outcome" => json!(result.outcome),
                _ => json!(result.code),
            };
            if compare(&actual, text(route, "operator"), &route["value"]) {
                return text(route, "id").into();
            }
        }
    }
    "out".into()
}
pub(crate) fn find_join(flow: &Workflow, starts: &[String]) -> Option<String> {
    if starts.is_empty() {
        return None;
    }
    let distances: Vec<_> = starts
        .iter()
        .map(|start| {
            let mut queue = std::collections::VecDeque::from([(start.clone(), 0usize)]);
            let mut seen = std::collections::HashMap::new();
            while let Some((id, distance)) = queue.pop_front() {
                if seen.contains_key(&id) {
                    continue;
                }
                seen.insert(id.clone(), distance);
                queue.extend(
                    flow.edges
                        .iter()
                        .filter(|e| e.source == id)
                        .map(|e| (e.target.clone(), distance + 1)),
                );
            }
            seen
        })
        .collect();
    flow.nodes
        .iter()
        .filter(|n| text(&n.data, "kind") == "join")
        .filter_map(|join| {
            let values: Option<Vec<_>> =
                distances.iter().map(|d| d.get(&join.id).copied()).collect();
            values.map(|v| {
                (
                    (
                        v.iter().copied().max().unwrap_or(0),
                        v.iter().sum::<usize>(),
                    ),
                    join.id.clone(),
                )
            })
        })
        .min_by_key(|(cost, _)| *cost)
        .map(|(_, id)| id)
}
