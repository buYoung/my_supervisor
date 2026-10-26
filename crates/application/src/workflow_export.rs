use crate::{
    workflow::{command_line, number, shell_quote, text, Workflow},
    AppError, AppResult, OperationsFacade,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn case_function(
    name: &str,
    selector: &str,
    cases: &[String],
    defaults: &str,
    after: &str,
) -> String {
    format!("{name}() {{\n  {defaults}\n  case \"{selector}\" in\n    {}\n    *) : ;;\n  esac\n  {after}\n}}\n",cases.join("\n    "))
}
fn assignment(key: &str, value: impl ToString) -> String {
    format!("{key}={}", shell_quote(&value.to_string()))
}
impl OperationsFacade {
    pub async fn export_workflow(&self, name: &str, request: Value) -> AppResult<Value> {
        let shell = request["shell"].as_str().unwrap_or("bash");
        if !["bash", "zsh"].contains(&shell) {
            return Err(AppError::InvalidRequest("shell must be bash or zsh".into()));
        }
        let include_env = request["include_environment"].as_bool().unwrap_or(false);
        let root = request
            .get("definition")
            .filter(|v| !v.is_null())
            .cloned()
            .unwrap_or(self.get_workflow(name, None).await?["definition"].clone());
        let mut pending = vec![(name.to_owned(), root)];
        let mut graphs = Vec::new();
        let mut seen = BTreeSet::new();
        while let Some((name, value)) = pending.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            if seen.len() > 32 {
                return Err(AppError::InvalidRequest("too many nested job calls".into()));
            }
            let flow: Workflow = serde_json::from_value(value)
                .map_err(|e| AppError::InvalidRequest(e.to_string()))?;
            crate::workflow::validate(&flow)?;
            for node in &flow.nodes {
                if text(&node.data, "kind") == "job" {
                    let called = text(&node.data, "jobId");
                    pending.push((
                        called.to_owned(),
                        self.get_workflow(called, None).await?["definition"].clone(),
                    ));
                }
            }
            graphs.push((name, flow));
        }
        let aliases: HashMap<_, _> = graphs
            .iter()
            .enumerate()
            .map(|(i, (name, _))| (name.clone(), format!("j{i}")))
            .collect();
        let mut configs = Vec::new();
        let mut actions = Vec::new();
        let mut edges = Vec::new();
        let mut routes = Vec::new();
        let mut starts = Vec::new();
        let mut kinds = Vec::new();
        let mut plan = Vec::new();
        let mut required = BTreeSet::from([
            "awk", "ps", "date", "mktemp", "sleep", "cat", "mkdir", "mv", "rm", "rmdir",
        ]);
        for (job, flow) in &graphs {
            let alias = &aliases[job];
            let ids: HashMap<_, _> = flow
                .nodes
                .iter()
                .enumerate()
                .map(|(i, n)| (n.id.clone(), format!("n{i}")))
                .collect();
            let start = flow
                .nodes
                .iter()
                .find(|n| text(&n.data, "kind") == "trigger")
                .unwrap();
            starts.push(format!(
                "{alias}) printf '%s\\n' {} ;;",
                shell_quote(&ids[&start.id])
            ));
            for e in &flow.edges {
                edges.push(format!(
                    "{}) printf '%s\\n' {} ;;",
                    shell_quote(&format!("{alias}:{}:{}", ids[&e.source], e.source_handle)),
                    shell_quote(&ids[&e.target])
                ));
            }
            for node in &flow.nodes {
                let key = format!("{alias}:{}", ids[&node.id]);
                let data = &node.data;
                let kind = text(data, "kind");
                let circuit = &data["circuitBreaker"];
                let mut fields = BTreeMap::new();
                for (key, value) in [
                    ("MSV_KIND", kind.to_owned()),
                    ("MSV_LABEL", format!("{job} / {}", text(data, "label"))),
                    ("MSV_STRATEGY", text(data, "retryStrategy").to_owned()),
                    ("MSV_ON_ERROR", text(data, "onError").to_owned()),
                    ("MSV_SOURCE", text(data, "source").to_owned()),
                    ("MSV_OPERATOR", text(data, "operator").to_owned()),
                    ("MSV_VALUE", text(data, "value").to_owned()),
                    ("MSV_PATH", text(data, "path").to_owned()),
                    ("MSV_JOIN_MODE", text(data, "joinMode").to_owned()),
                    ("MSV_FAILURE_POLICY", text(data, "failurePolicy").to_owned()),
                ] {
                    fields.insert(key, value);
                }
                for (field, key, default) in [
                    ("MSV_TIMEOUT", "timeoutSeconds", 60.),
                    ("MSV_ATTEMPTS", "maxAttempts", 1.),
                    ("MSV_DELAY", "retryDelaySeconds", 1.),
                    ("MSV_INCREMENT", "retryIncrementSeconds", 1.),
                    ("MSV_MULTIPLIER", "retryMultiplier", 2.),
                    ("MSV_MAX_DELAY", "retryMaxDelaySeconds", 60.),
                ] {
                    fields.insert(field, number(data, key, default).to_string());
                }
                fields.insert(
                    "MSV_CIRCUIT",
                    if circuit["enabled"] == true { "1" } else { "0" }.into(),
                );
                fields.insert(
                    "MSV_JITTER",
                    if data["retryJitter"] == true {
                        "1"
                    } else {
                        "0"
                    }
                    .into(),
                );
                fields.insert(
                    "MSV_CIRCUIT_KEY",
                    format!("{}-{}", aliases[job], ids[&node.id]),
                );
                fields.insert(
                    "MSV_THRESHOLD",
                    number(circuit, "failureThreshold", 3.).to_string(),
                );
                fields.insert(
                    "MSV_OPEN_SECONDS",
                    number(circuit, "openSeconds", 60.).to_string(),
                );
                fields.insert(
                    "MSV_RECOVERY",
                    number(circuit, "recoverySuccesses", 1.).to_string(),
                );
                fields.insert("MSV_PROBE_LEASE", "90000".into());
                let retry = data["retryOn"].as_array();
                fields.insert(
                    "MSV_RETRY_ON",
                    if retry.is_some_and(|values| values.len() == 1 && values[0] == "timed_out") {
                        "timeout"
                    } else if retry.is_some_and(|values| values.len() == 1 && values[0] == "failed")
                    {
                        "failure"
                    } else {
                        "all"
                    }
                    .into(),
                );
                let node_routes = data["resultRoutes"].as_array();
                fields.insert(
                    "MSV_HAS_ROUTES",
                    if node_routes.is_some_and(|r| !r.is_empty()) {
                        "1"
                    } else {
                        "0"
                    }
                    .into(),
                );
                fields.insert(
                    "MSV_BRANCHES",
                    flow.edges
                        .iter()
                        .filter(|e| e.source == node.id)
                        .map(|e| e.source_handle.clone())
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                let branches = flow
                    .edges
                    .iter()
                    .filter(|e| e.source == node.id)
                    .map(|e| e.target.clone())
                    .collect::<Vec<_>>();
                fields.insert(
                    "MSV_JOIN",
                    crate::workflow_engine::find_join(flow, &branches)
                        .and_then(|id| ids.get(&id).cloned())
                        .unwrap_or_default(),
                );
                configs.push(format!(
                    "{})\n      {}\n      ;;",
                    shell_quote(&key),
                    fields
                        .into_iter()
                        .map(|(k, v)| assignment(k, v))
                        .collect::<Vec<_>>()
                        .join("\n      ")
                ));
                kinds.push(format!(
                    "{}) printf '%s\\n' {} ;;",
                    shell_quote(&key),
                    shell_quote(
                        if kind == "condition" && text(data, "source") == "exit_code" {
                            "condition_exit"
                        } else {
                            kind
                        }
                    )
                ));
                if let Some(items) = node_routes {
                    let lines=items.iter().map(|r|format!("if msv_match {} {} {} \"$3\" \"$4\" \"$5\"; then printf '%s\\n' {}; return; fi",shell_quote(text(r,"source")),shell_quote(text(r,"operator")),shell_quote(text(r,"value")),shell_quote(text(r,"id")))).collect::<Vec<_>>();
                    routes.push(format!("{}) {} ;;", shell_quote(&key), lines.join("\n")));
                }
                let mut body = Vec::new();
                match kind {
                    "command" => {
                        if !text(data, "cwd").is_empty() {
                            body.push(format!(
                                "cd -- {} || return 1",
                                shell_quote(text(data, "cwd"))
                            ));
                        }
                        let mut env = Vec::new();
                        if let Some(values) = data["environment"].as_object() {
                            for (k, v) in values {
                                if !k.chars().enumerate().all(|(i, c)| {
                                    c == '_'
                                        || c.is_ascii_alphabetic()
                                        || i > 0 && c.is_ascii_digit()
                                }) {
                                    return Err(AppError::InvalidRequest(
                                        "invalid environment name".into(),
                                    ));
                                }
                                if include_env {
                                    env.push(shell_quote(&format!(
                                        "{k}={}",
                                        v.as_str().unwrap_or("")
                                    )));
                                } else {
                                    body.push(format!(": \"${{{k}:?set {k} before running}}\""));
                                    env.push(format!("{}\"${k}\"", shell_quote(&format!("{k}="))));
                                }
                            }
                        }
                        let args = data["args"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(Value::as_str)
                                    .map(str::to_owned)
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let command = if text(data, "program").is_empty() {
                            format!("\"$MSV_SHELL\" -c {}", shell_quote(text(data, "command")))
                        } else {
                            command_line(text(data, "program"), &args)
                        };
                        if env.is_empty() {
                            body.push(command);
                        } else {
                            required.insert("env");
                            body.push(format!("env {} {command}", env.join(" ")));
                        }
                    }
                    "process" => {
                        required.insert("msv");
                        body.push(format!(
                            "\"$MSV_CLI\" {} {}",
                            shell_quote(text(data, "operation")),
                            shell_quote(text(data, "processName"))
                        ));
                    }
                    "health" => {
                        if text(data, "checkType") == "tcp" {
                            required.insert("nc");
                            body.push(format!(
                                "nc -z -w {} -- {} {}",
                                number(data, "timeoutSeconds", 10.),
                                shell_quote(text(data, "host")),
                                number(data, "port", 80.)
                            ));
                        } else {
                            required.insert("curl");
                            body.push(format!("local status_code; status_code=$(curl --silent --show-error --output /dev/null --write-out '%{{http_code}}' --max-time {} -- {}) || return $?; [ \"$status_code\" = {} ]",number(data,"timeoutSeconds",10.),shell_quote(text(data,"url")),number(data,"expectedStatus",200.)));
                        }
                    }
                    "wait" => body.push(format!("sleep {}", number(data, "seconds", 1.))),
                    "job" => {
                        let target = &aliases[text(data, "jobId")];
                        body.push(format!(
                            "msv_path {target} \"$(msv_start {target})\" '' 0 \"$4\" \"$5\""
                        ));
                    }
                    "finish" => body.push(format!(
                        "printf '%s\\n' {}; return {}",
                        shell_quote(text(data, "message")),
                        if text(data, "result") == "failed" {
                            1
                        } else {
                            0
                        }
                    )),
                    _ => body.push("return 0".into()),
                }
                actions.push(format!(
                    "{})\n      {}\n      ;;",
                    shell_quote(&key),
                    body.join("\n      ")
                ));
                plan.push(format!(
                    "printf '%s\\n' {}",
                    shell_quote(&format!("{job} / {} [{kind}]", text(data, "label")))
                ));
            }
        }
        let defaults="MSV_KIND=''; MSV_LABEL=''; MSV_TIMEOUT=60; MSV_ATTEMPTS=1; MSV_DELAY=1; MSV_STRATEGY=fixed; MSV_INCREMENT=1; MSV_MULTIPLIER=2; MSV_MAX_DELAY=60; MSV_JITTER=0; MSV_RETRY_ON=all; MSV_CIRCUIT=0; MSV_CIRCUIT_KEY=''; MSV_THRESHOLD=3; MSV_OPEN_SECONDS=60; MSV_RECOVERY=1; MSV_PROBE_LEASE=90000; MSV_HAS_ROUTES=0; MSV_ON_ERROR=stop; MSV_SOURCE=''; MSV_OPERATOR=equals; MSV_VALUE=0; MSV_PATH=''; MSV_BRANCHES=''; MSV_JOIN=''; MSV_JOIN_MODE=all; MSV_FAILURE_POLICY=fail";
        let mut script=format!("#!/usr/bin/env {shell}\n{}\n# my_supervisor workflow export: {}\n# Run once; schedules are not installed. Use --dry-run to inspect.\numask 077\nMSV_SHELL=${{MSV_SHELL:-/bin/{shell}}}\nMSV_CLI=${{MSV_CLI:-msv}}\nMSV_STATE_DIR=${{MSV_STATE_DIR:-\"$HOME/.local/state/my-supervisor/exports/{}\"}}\nTMPDIR=${{TMPDIR:-/tmp}}\nMSV_ROOTS=j0\nMSV_MAX_PARALLEL=1\nMSV_GROUP_FAILURE=stop\nMSV_GROUP_TIMEOUT=0\n{}\nMSV_TMP=''; MSV_MAIN_PID=''; MSV_TIMER_PID=''\n",if shell=="zsh"{"emulate -R sh"}else{""},name.replace(['\r','\n']," "),self.get_job(name).await?.job.id.0,assignment("MSV_REQUIREMENTS",required.into_iter().collect::<Vec<_>>().join(" ")));
        script.push_str(&case_function("msv_action", "$1:$2", &actions, "", ""));
        script.push_str(&case_function(
            "msv_config",
            "$1:$2",
            &configs,
            defaults,
            "",
        ));
        script.push_str(&case_function("msv_edge", "$1:$2:$3", &edges, "", ""));
        script.push_str(&case_function(
            "msv_route",
            "$1:$2",
            &routes,
            "",
            "printf '%s\\n' out",
        ));
        script.push_str(&case_function("msv_start", "$1", &starts, "", ""));
        script.push_str(&case_function("msv_kind", "$1:$2", &kinds, "", ""));
        script.push_str("msv_parents() { :; }\nmsv_dependency_policy() { printf '%s\\n' skip; }\nmsv_plan() {\n");
        script.push_str(&plan.join("\n"));
        script.push_str("\n}\n");
        script.push_str(include_str!("workflow-runtime.sh"));
        script.push_str("\nmsv_main \"$1\"\nexit $?\n");
        Ok(json!({"script":script,"shell":shell,"includes_environment":include_env}))
    }
}
