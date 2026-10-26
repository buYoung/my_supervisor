
msv_log() { printf '[%s] %s\n' "$(date '+%H:%M:%S')" "$*" >&2; }
msv_children() { ps -ax -o pid= -o ppid= | awk -v parent="$1" '$2 == parent { print $1 }'; }
msv_stop() {
  local child
  for child in $(msv_children "$1"); do msv_stop "$child"; done
  kill -TERM "$1" 2>/dev/null || :
  kill -KILL "$1" 2>/dev/null || :
}
msv_lock() {
  local count=0 expires now
  while ! mkdir "$1" 2>/dev/null; do
    expires=0; now=$(date +%s)
    if [ -f "$1/lease" ]; then
      read -r expires < "$1/lease"
      if [ "$expires" -gt 0 ] && [ "$expires" -lt "$now" ]; then rm -f "$1/lease"; rmdir "$1" 2>/dev/null || :; fi
    fi
    count=$((count+1)); [ "$count" -lt 100 ] || return 1
    sleep 0.05
  done
  printf '%s\n' "$(($(date +%s)+15))" > "$1/lease"
}
msv_unlock() { rm -f "$1/lease"; rmdir "$1"; }
msv_read_circuit() {
  circuit_failures=0; circuit_until=0; circuit_recovered=0
  if [ -f "$1/state" ]; then read -r circuit_failures circuit_until circuit_recovered < "$1/state"; fi
}
msv_circuit_enter() {
  local directory="$1" now lease
  mkdir -p "$directory" || return 1
  msv_lock "$directory/lock" || return 1
  msv_read_circuit "$directory"
  now=$(date +%s); MSV_IS_PROBE=0
  if [ "$circuit_until" -gt "$now" ]; then msv_unlock "$directory/lock"; return 1; fi
  if [ "$circuit_until" -gt 0 ]; then
    lease=0; [ ! -f "$directory/probe" ] || read -r lease < "$directory/probe"
    if [ "$lease" -gt "$now" ]; then msv_unlock "$directory/lock"; return 1; fi
    printf '%s\n' "$((now+MSV_PROBE_LEASE))" > "$directory/probe"
    MSV_IS_PROBE=1
  fi
  msv_unlock "$directory/lock"
}
msv_circuit_leave() {
  local directory="$1" code="$2" probe="$3"
  msv_lock "$directory/lock" || return 1
  msv_read_circuit "$directory"
  if [ "$code" -eq 0 ]; then
    circuit_failures=0
    if [ "$probe" -eq 1 ]; then
      circuit_recovered=$((circuit_recovered+1))
      if [ "$circuit_recovered" -ge "$MSV_RECOVERY" ]; then circuit_until=0; circuit_recovered=0; fi
    elif [ "$circuit_until" -eq 0 ]; then circuit_recovered=0; fi
  else
    circuit_failures=$((circuit_failures+1)); circuit_recovered=0
    if [ "$probe" -eq 1 ] || [ "$circuit_failures" -ge "$MSV_THRESHOLD" ]; then circuit_until=$(($(date +%s)+MSV_OPEN_SECONDS)); fi
  fi
  printf '%s %s %s\n' "$circuit_failures" "$circuit_until" "$circuit_recovered" > "$directory/state.next"
  mv "$directory/state.next" "$directory/state"
  [ "$probe" -ne 1 ] || rm -f "$directory/probe"
  msv_unlock "$directory/lock"
}
msv_retry_delay() {
  awk -v base="$MSV_DELAY" -v strategy="$MSV_STRATEGY" -v attempt="$1" -v increment="$MSV_INCREMENT" -v factor="$MSV_MULTIPLIER" -v cap="$MSV_MAX_DELAY" -v jitter="$MSV_JITTER" -v seed="$(date +%s)" 'BEGIN {
    value=base;
    if(strategy=="linear") value=base+(attempt-1)*increment;
    if(strategy=="exponential") value=base*(factor^(attempt-1));
    if(value>cap) value=cap;
    if(jitter==1) { srand(seed+attempt); value=value*(0.8+rand()*0.4); }
    printf "%.3f",value;
  }'
}
msv_policy() {
  local job="$1" node="$2" incoming="$3" directory="$4" depth="$5"
  local attempt=1 code=0 pid timer='' probe=0 circuit delay_seconds
  msv_config "$job" "$node"; mkdir -p "$directory"
  circuit="$MSV_STATE_DIR/$MSV_CIRCUIT_KEY"
  MSV_OUTCOME=succeeded
  if [ "$MSV_CIRCUIT" -eq 1 ]; then
    if ! msv_circuit_enter "$circuit"; then
      msv_log "$MSV_LABEL: 서킷 차단 또는 복구 확인 중"
      MSV_OUTCOME=circuit_open; : > "$directory/stdout"; : > "$directory/stderr"; return 125
    fi
    probe=$MSV_IS_PROBE
  fi
  while :; do
    : > "$directory/stdout"; : > "$directory/stderr"; rm -f "$directory/timed-out"
    msv_log "$MSV_LABEL · 시도 $attempt/$MSV_ATTEMPTS"
    ( msv_action "$job" "$node" "$incoming" "$directory/child" "$depth" ) > "$directory/stdout" 2> "$directory/stderr" &
    pid=$!
    if [ "$MSV_TIMEOUT" -gt 0 ]; then
      ( sleep "$MSV_TIMEOUT"; if kill -0 "$pid" 2>/dev/null; then : > "$directory/timed-out"; msv_stop "$pid"; fi ) &
      timer=$!
    fi
    wait "$pid"; code=$?
    if [ -n "$timer" ]; then msv_stop "$timer"; wait "$timer" 2>/dev/null || :; timer=''; fi
    MSV_OUTCOME=failed; [ "$code" -ne 0 ] || MSV_OUTCOME=succeeded
    if [ -f "$directory/timed-out" ]; then code=124; MSV_OUTCOME=timeout; printf '%s\n' '단계 제한 시간 초과' >> "$directory/stderr"; fi
    cat "$directory/stdout"; cat "$directory/stderr" >&2
    if [ "$code" -eq 0 ] || [ "$attempt" -ge "$MSV_ATTEMPTS" ]; then break; fi
    if [ "$MSV_RETRY_ON" = timeout ] && [ "$MSV_OUTCOME" != timeout ]; then break; fi
    if [ "$MSV_RETRY_ON" = failure ] && [ "$MSV_OUTCOME" = timeout ]; then break; fi
    delay_seconds=$(msv_retry_delay "$attempt")
    msv_log "$MSV_LABEL: $delay_seconds 초 후 재시도"
    sleep "$delay_seconds"; attempt=$((attempt+1))
  done
  if [ "$MSV_CIRCUIT" -eq 1 ]; then msv_circuit_leave "$circuit" "$code" "$probe" || msv_log '서킷 상태 저장 실패'; fi
  return "$code"
}
msv_compare() {
  local actual="$1" operator="$2" expected="$3"
  case "$operator" in
    equals) [ "$actual" = "$expected" ] ;;
    not_equals) [ "$actual" != "$expected" ] ;;
    gte) [ "$actual" -ge "$expected" ] 2>/dev/null ;;
    lte) [ "$actual" -le "$expected" ] 2>/dev/null ;;
    includes|contains) case "$actual" in *"$expected"*) return 0;; *) return 1;; esac ;;
    not_includes) case "$actual" in *"$expected"*) return 1;; *) return 0;; esac ;;
    *) return 1 ;;
  esac
}
msv_match() {
  local source="$1" operator="$2" expected="$3" code="$4" outcome="$5" directory="$6" actual
  case "$source" in
    exit_code) [ "$outcome" != circuit_open ] || return 1; actual="$code" ;;
    outcome) actual="$outcome" ;;
    stdout|stderr) actual=$(cat "$directory/$source") ;;
    *) return 1 ;;
  esac
  msv_compare "$actual" "$operator" "$expected"
}
msv_parallel() {
  local job="$1" fork="$2" incoming="$3" directory="$4" depth="$5"
  local branches join mode policy branch target pid code winner='' winner_code=1 remaining failed=0
  msv_config "$job" "$fork"; branches="$MSV_BRANCHES"; join="$MSV_JOIN"
  msv_config "$job" "$join"; mode="$MSV_JOIN_MODE"; policy="$MSV_FAILURE_POLICY"
  mkdir -p "$directory"
  for branch in $branches; do
    target=$(msv_edge "$job" "$fork" "$branch")
    (
      msv_path "$job" "$target" "$join" "$incoming" "$directory/$branch" "$depth"
      code=$?
      if [ "$mode" = race ] || { [ "$mode" = first_success ] && [ "$code" -eq 0 ]; }; then
        if mkdir "$directory/winner-lock" 2>/dev/null; then printf '%s %s\n' "$branch" "$code" > "$directory/winner.next"; mv "$directory/winner.next" "$directory/winner"; fi
      fi
      printf '%s\n' "$code" > "$directory/$branch.next"; mv "$directory/$branch.next" "$directory/$branch.done"
    ) &
    printf '%s\n' "$!" > "$directory/$branch.pid"
  done
  while :; do
    if [ -f "$directory/winner" ]; then
      read -r winner winner_code < "$directory/winner"
      for branch in $branches; do
        read -r pid < "$directory/$branch.pid"
        if [ "$branch" != "$winner" ] && [ ! -f "$directory/$branch.done" ]; then msv_stop "$pid"; msv_log "$branch: 다른 분기 결과 채택으로 취소"; fi
        wait "$pid" 2>/dev/null || :
      done
      msv_log "$mode: $winner 결과 채택 · 종료 $winner_code"
      return "$winner_code"
    fi
    remaining=0
    for branch in $branches; do [ -f "$directory/$branch.done" ] || remaining=$((remaining+1)); done
    if [ "$remaining" -eq 0 ]; then
      [ ! -f "$directory/winner" ] || continue
      break
    fi
    sleep 0.05
  done
  for branch in $branches; do
    read -r pid < "$directory/$branch.pid"; wait "$pid" 2>/dev/null || :
    read -r code < "$directory/$branch.done"; [ "$code" -eq 0 ] || failed=$((failed+1))
  done
  [ "$failed" -eq 0 ] || { [ "$mode" = all ] && [ "$policy" = collect ]; }
}
msv_path() {
  local job="$1" node="$2" stop="$3" incoming="$4" directory="$5" depth="$6"
  local kind next port code=0 count=0 action_dir label previous_dir
  depth=$((depth+1)); [ "$depth" -le 32 ] || { msv_log '작업 호출 깊이 초과'; return 1; }
  mkdir -p "$directory"
  while [ -n "$node" ]; do
    if [ "$node" = "$stop" ]; then [ "$incoming" != circuit_open ] || return 125; return "$incoming"; fi
    count=$((count+1)); [ "$count" -le 1000 ] || { msv_log '반복 경로 차단'; return 1; }
    msv_config "$job" "$node"; kind="$MSV_KIND"; label="$MSV_LABEL"; action_dir="$directory/$node"
    case "$kind" in
      condition)
        port=false
        if [ "$MSV_SOURCE" = file_exists ]; then [ ! -e "$MSV_PATH" ] || port=true
        elif [ "$MSV_SOURCE" = stdout ] || [ "$MSV_SOURCE" = stderr ]; then
          if [ -n "$previous_dir" ] && msv_compare "$(cat "$previous_dir/$MSV_SOURCE")" "$MSV_OPERATOR" "$MSV_VALUE"; then port=true; fi
        elif [ "$incoming" != circuit_open ] && msv_compare "$incoming" "$MSV_OPERATOR" "$MSV_VALUE"; then port=true; fi
        msv_log "$label: $port"; node=$(msv_edge "$job" "$node" "$port"); incoming=0; continue ;;
      parallel)
        next="$MSV_JOIN"; msv_parallel "$job" "$node" "$incoming" "$action_dir" "$depth"; incoming=$?
        [ -n "$next" ] || return "$incoming"
        node="$next"; continue ;;
      join)
        code="$incoming"; MSV_OUTCOME=failed; [ "$code" -ne 0 ] || MSV_OUTCOME=succeeded
        mkdir -p "$action_dir"; : > "$action_dir/stdout"; : > "$action_dir/stderr" ;;
      *) msv_policy "$job" "$node" "$incoming" "$action_dir" "$depth"; code=$? ;;
    esac
    msv_config "$job" "$node"
    [ "$kind" != finish ] || return "$code"
    port=$(msv_route "$job" "$node" "$code" "$MSV_OUTCOME" "$action_dir")
    next=$(msv_edge "$job" "$node" "$port")
    if [ "$code" -ne 0 ] && [ "$MSV_HAS_ROUTES" -eq 0 ] && [ "$MSV_ON_ERROR" != continue ]; then
      if [ -z "$next" ] || [ "$(msv_kind "$job" "$next")" != condition_exit ]; then return "$code"; fi
    fi
    previous_dir="$action_dir"
    incoming="$code"; [ "$MSV_OUTCOME" != circuit_open ] || incoming=circuit_open
    node="$next"
  done
  msv_log '다음 실행 경로 없음'; return "$code"
}
msv_group() {
  local directory="$1" job parent state pid code count started remaining blocked any_failed=0 halt=0
  mkdir -p "$directory"
  for job in $MSV_ROOTS; do printf '%s\n' pending > "$directory/$job.state"; done
  while :; do
    count=0; remaining=0; started=0
    for job in $MSV_ROOTS; do
      read -r state < "$directory/$job.state"
      if [ "$state" = running ]; then
        if [ -f "$directory/$job.done" ]; then
          read -r code < "$directory/$job.done"; read -r pid < "$directory/$job.pid"; wait "$pid" 2>/dev/null || :
          if [ "$code" -eq 0 ]; then state=succeeded; else state=failed; any_failed=1; [ "$MSV_GROUP_FAILURE" != stop ] || halt=1; fi
          printf '%s\n' "$state" > "$directory/$job.state"; msv_log "$job: $state"
        else count=$((count+1)); fi
      fi
    done
    for job in $MSV_ROOTS; do
      read -r state < "$directory/$job.state"; [ "$state" = pending ] || continue
      blocked=0
      for parent in $(msv_parents "$job"); do
        read -r state < "$directory/$parent.state"
        case "$state" in pending|running) blocked=1;; failed|skipped) if [ "$(msv_dependency_policy "$job")" != run_anyway ]; then blocked=2; break; fi;; esac
      done
      if [ "$halt" -eq 1 ] || [ "$blocked" -eq 2 ]; then printf '%s\n' skipped > "$directory/$job.state"; any_failed=1; continue; fi
      remaining=$((remaining+1))
      if [ "$blocked" -eq 0 ] && [ "$count" -lt "$MSV_MAX_PARALLEL" ]; then
        (
          msv_path "$job" "$(msv_start "$job")" '' 0 "$directory/$job" 0
          code=$?; printf '%s\n' "$code" > "$directory/$job.next"; mv "$directory/$job.next" "$directory/$job.done"
        ) &
        printf '%s\n' "$!" > "$directory/$job.pid"; printf '%s\n' running > "$directory/$job.state"
        count=$((count+1)); started=$((started+1))
      fi
    done
    if [ "$count" -eq 0 ]; then
      [ "$remaining" -gt 0 ] || break
      if [ "$started" -eq 0 ]; then msv_log '실행 가능한 작업 없음 · 선행 관계 확인 필요'; return 1; fi
    fi
    sleep 0.05
  done
  return "$any_failed"
}
msv_cleanup() {
  local pid
  [ -z "$MSV_MAIN_PID" ] || msv_stop "$MSV_MAIN_PID"
  [ -z "$MSV_TIMER_PID" ] || msv_stop "$MSV_TIMER_PID"
  [ -z "$MSV_TMP" ] || rm -rf "$MSV_TMP"
}
msv_main() {
  local required code
  if [ "$1" = --dry-run ]; then msv_plan; return 0; fi
  if [ -n "$1" ]; then printf '%s\n' '사용법: 실행 파일 [--dry-run]' >&2; return 2; fi
  for required in $MSV_REQUIREMENTS; do
    [ "$required" != msv ] || required="$MSV_CLI"
    command -v "$required" >/dev/null 2>&1 || { msv_log "필요한 명령 없음: $required"; return 127; }
  done
  MSV_TMP=$(mktemp -d "$TMPDIR/msv-export.XXXXXXXX") || return 1
  trap 'msv_cleanup' EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM HUP
  msv_group "$MSV_TMP/group" & MSV_MAIN_PID=$!
  if [ "$MSV_GROUP_TIMEOUT" -gt 0 ]; then
    ( sleep "$MSV_GROUP_TIMEOUT"; if kill -0 "$MSV_MAIN_PID" 2>/dev/null; then : > "$MSV_TMP/timed-out"; msv_stop "$MSV_MAIN_PID"; fi ) & MSV_TIMER_PID=$!
  fi
  wait "$MSV_MAIN_PID"; code=$?; MSV_MAIN_PID=''
  if [ -n "$MSV_TIMER_PID" ]; then msv_stop "$MSV_TIMER_PID"; wait "$MSV_TIMER_PID" 2>/dev/null || :; MSV_TIMER_PID=''; fi
  [ ! -f "$MSV_TMP/timed-out" ] || { msv_log '그룹 제한 시간 초과'; code=124; }
  return "$code"
}
