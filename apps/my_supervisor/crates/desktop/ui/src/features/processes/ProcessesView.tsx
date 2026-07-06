import { ArrowRightLeft, Check, Copy, Pause, Play, Plus, RotateCcw, Trash2 } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { Badge, Button, DataTable, EmptyState, Field, IconButton, Panel, PanelHeader, TableCell } from "../../components/ui/primitives";
import { usePolledResource, useOperationsClient, toErrorMessage } from "../../services/use-operations";
import type { ProcessConfigDto } from "../../services/wire-types";
import type { ProcessState } from "../../shared/types";

const PROCESSES_POLL_INTERVAL_MS = 2000;

type ServicePlatform = "macos" | "linux" | "windows";

const stateTone: Record<ProcessState, "success" | "warning" | "danger" | "info" | "neutral"> = {
  starting: "info",
  running: "success",
  stopping: "warning",
  crashed: "danger",
  stopped: "neutral",
};

function formatMemory(memoryBytes: number) {
  if (memoryBytes === 0) {
    return "-";
  }

  return `${(memoryBytes / 1024 / 1024).toFixed(1)} MiB`;
}

const servicePlatformLabels: Record<ServicePlatform, string> = {
  macos: "macOS launchd",
  linux: "Linux systemd",
  windows: "Windows Service",
};

function detectServicePlatform(): ServicePlatform {
  const userAgent = navigator.userAgent.toLowerCase();
  const platform = navigator.platform.toLowerCase();

  if (userAgent.includes("windows") || platform.includes("win")) {
    return "windows";
  }

  if (userAgent.includes("linux") || platform.includes("linux")) {
    return "linux";
  }

  return "macos";
}

function getServiceRegistrationPreview(servicePlatform: ServicePlatform) {
  if (servicePlatform === "linux") {
    return {
      commandTitle: "등록 명령",
      command:
        "mkdir -p ~/.config/systemd/user\ncp my-supervisor-managed-api-server.service ~/.config/systemd/user/\nsystemctl --user daemon-reload\nsystemctl --user enable --now my-supervisor-managed-api-server.service",
      configPath: "~/.config/systemd/user/my-supervisor-managed-api-server.service",
      config: `[Unit]
Description=my-supervisor managed process: api-server
After=network.target

[Service]
Type=simple
WorkingDirectory=/Users/buyonglee/projects/api
ExecStart=/bin/sh -lc 'pnpm dev'
Restart=on-failure
RestartSec=3
Environment=MYSUPERVISOR_PROCESS_NAME=api-server

[Install]
WantedBy=default.target`,
    };
  }

  if (servicePlatform === "windows") {
    return {
      commandTitle: "등록 명령",
      command:
        'New-Service -Name "my-supervisor-managed-api-server" -DisplayName "my-supervisor api-server" -BinaryPathName "cmd.exe /c cd /d C:\\Users\\buyonglee\\projects\\api && pnpm dev" -StartupType Automatic\nStart-Service "my-supervisor-managed-api-server"',
      configPath: "PowerShell 등록 스크립트",
      config: `# register-api-server.ps1
$serviceName = "my-supervisor-managed-api-server"
$displayName = "my-supervisor api-server"
$workingDirectory = "C:\\Users\\buyonglee\\projects\\api"
$command = "pnpm dev"

New-Service \`
  -Name $serviceName \`
  -DisplayName $displayName \`
  -BinaryPathName "cmd.exe /c cd /d $workingDirectory && $command" \`
  -StartupType Automatic

Start-Service $serviceName`,
    };
  }

  return {
    commandTitle: "등록 명령",
    command:
      "cp com.my-supervisor.managed.api-server.plist ~/Library/LaunchAgents/\nlaunchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.my-supervisor.managed.api-server.plist\nlaunchctl enable gui/$(id -u)/com.my-supervisor.managed.api-server",
    configPath: "~/Library/LaunchAgents/com.my-supervisor.managed.api-server.plist",
    config: `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.my-supervisor.managed.api-server</string>
  <key>WorkingDirectory</key>
  <string>/Users/buyonglee/projects/api</string>
  <key>ProgramArguments</key>
  <array>
    <string>/bin/sh</string>
    <string>-lc</string>
    <string>pnpm dev</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>/Users/buyonglee/Library/Logs/my-supervisor/api-server.out.log</string>
  <key>StandardErrorPath</key>
  <string>/Users/buyonglee/Library/Logs/my-supervisor/api-server.err.log</string>
</dict>
</plist>`,
  };
}

export function ProcessesView() {
  const client = useOperationsClient();
  const [selectedManagementMode, setSelectedManagementMode] = useState<"direct" | "system_registered">("direct");
  const [copiedSnippet, setCopiedSnippet] = useState<"command" | "config" | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [pendingProcess, setPendingProcess] = useState<string | null>(null);
  const [formName, setFormName] = useState("api-server");
  const [formCommand, setFormCommand] = useState("pnpm dev");
  const [formCwd, setFormCwd] = useState("/Users/buyonglee/projects/api");
  const servicePlatform = useMemo(() => detectServicePlatform(), []);
  const serviceRegistrationPreview = useMemo(
    () => getServiceRegistrationPreview(servicePlatform),
    [servicePlatform],
  );

  const listProcesses = useCallback(() => client.listProcesses(), [client]);
  const { data: processes, isLoading, errorMessage, refresh } = usePolledResource(
    listProcesses,
    PROCESSES_POLL_INTERVAL_MS,
  );

  const runProcessAction = useCallback(
    async (name: string, action: () => Promise<void>) => {
      setActionError(null);
      setPendingProcess(name);
      try {
        await action();
        await refresh();
      } catch (error) {
        setActionError(toErrorMessage(error));
      } finally {
        setPendingProcess(null);
      }
    },
    [refresh],
  );

  const handleConvert = useCallback(
    (name: string, currentMode: "direct" | "system_registered") => {
      const target = currentMode === "direct" ? "system_registered" : "direct";
      return runProcessAction(name, async () => {
        // Real convert (child 06): on macOS this writes/boots a launchd
        // LaunchAgent; a failed registration rolls back to the prior mode.
        await client.convertProcess(name, target, {
          autoStart: target === "system_registered",
        });
      });
    },
    [client, runProcessAction],
  );

  const handleAddProcess = useCallback(async () => {
    setActionError(null);
    if (!formName.trim() || !formCommand.trim()) {
      setActionError("이름과 명령은 필수 항목입니다.");
      return;
    }
    setPendingProcess(formName);
    const config: ProcessConfigDto = {
      name: formName.trim(),
      command: formCommand.trim(),
      ...(formCwd.trim() ? { cwd: formCwd.trim() } : {}),
      management_mode:
        selectedManagementMode === "system_registered"
          ? { type: "system_registered", unit_name: `my-supervisor-managed-${formName.trim()}` }
          : { type: "direct" },
    };
    try {
      await client.addProcess(config);
      await refresh();
    } catch (error) {
      setActionError(toErrorMessage(error));
    } finally {
      setPendingProcess(null);
    }
  }, [client, formName, formCommand, formCwd, selectedManagementMode, refresh]);

  const processList = processes ?? [];
  const runningCount = processList.filter((process) => process.state === "running").length;
  const systemRegisteredCount = processList.filter(
    (process) => process.managementMode.type === "system_registered",
  ).length;
  const crashedCount = processList.filter((process) => process.state === "crashed").length;

  const copySnippet = async (snippetType: "command" | "config", value: string) => {
    await navigator.clipboard.writeText(value);
    setCopiedSnippet(snippetType);
    window.setTimeout(() => setCopiedSnippet(null), 1600);
  };

  return (
    <div className="grid gap-5 xl:grid-cols-[minmax(0,1fr)_360px]">
      <div className="grid gap-5">
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          <Panel className="p-4">
            <p className="text-xs font-medium text-muted">전체 프로세스</p>
            <p className="mt-2 text-2xl font-semibold text-foreground">{processList.length}</p>
          </Panel>
          <Panel className="p-4">
            <p className="text-xs font-medium text-muted">실행 중</p>
            <p className="mt-2 text-2xl font-semibold text-success">{runningCount}</p>
          </Panel>
          <Panel className="p-4">
            <p className="text-xs font-medium text-muted">시스템 등록</p>
            <p className="mt-2 text-2xl font-semibold text-foreground">{systemRegisteredCount}</p>
          </Panel>
          <Panel className="p-4">
            <p className="text-xs font-medium text-muted">주의 필요</p>
            <p className="mt-2 text-2xl font-semibold text-danger">{crashedCount}</p>
          </Panel>
        </div>

        <Panel>
          <PanelHeader
            action={
              <Button variant="primary" disabled={pendingProcess !== null} onClick={handleAddProcess}>
                <Plus aria-hidden="true" size={16} />
                프로세스 추가
              </Button>
            }
            description="데몬이 관리하는 프로세스 목록입니다. 2초마다 자동으로 갱신됩니다."
            title="프로세스"
          />
          {actionError ? (
            <div className="border-b border-border bg-danger/10 px-4 py-2 text-xs font-medium text-danger">
              {actionError}
            </div>
          ) : null}
          {isLoading ? (
            <div className="px-4 py-6 text-sm text-muted">불러오는 중…</div>
          ) : errorMessage && processList.length === 0 ? (
            <div className="p-4">
              <EmptyState title="데몬에 연결할 수 없습니다" description={errorMessage} />
            </div>
          ) : processList.length === 0 ? (
            <div className="p-4">
              <EmptyState
                title="등록된 프로세스가 없습니다"
                description="우측 폼에서 새 프로세스를 추가해 보세요."
              />
            </div>
          ) : (
            <DataTable columns={["이름", "상태", "관리 모드", "PID 또는 유닛", "업타임", "자원", "동작"]}>
              {processList.map((process) => (
                <tr className="transition-colors duration-200 hover:bg-surface" key={process.name}>
                  <TableCell>
                    <div className="font-medium text-foreground">{process.name}</div>
                    <div className="text-xs text-muted">재시작 {process.restartCount}회</div>
                  </TableCell>
                  <TableCell>
                    <Badge tone={stateTone[process.state]}>{process.state}</Badge>
                  </TableCell>
                  <TableCell>
                    <Badge tone={process.managementMode.type === "direct" ? "info" : "warning"}>
                      {process.managementMode.type === "direct" ? "Direct" : "System"}
                    </Badge>
                  </TableCell>
                  <TableCell>
                    <span className="font-mono text-xs text-muted">
                      {process.managementMode.type === "direct"
                        ? process.pid ?? "-"
                        : process.managementMode.unitName}
                    </span>
                  </TableCell>
                  <TableCell>{process.uptime}</TableCell>
                  <TableCell>
                    <div className="text-xs text-muted">
                      <span className="text-foreground">{process.cpuPercent.toFixed(1)}%</span> CPU
                    </div>
                    <div className="text-xs text-muted">
                      <span className="text-foreground">{formatMemory(process.memoryBytes)}</span> 메모리
                    </div>
                  </TableCell>
                  <TableCell>
                    <div className="flex items-center gap-2">
                      <IconButton
                        label="시작"
                        disabled={pendingProcess === process.name}
                        onClick={() => runProcessAction(process.name, () => client.startProcess(process.name))}
                      >
                        <Play aria-hidden="true" size={15} />
                      </IconButton>
                      <IconButton
                        label="중지"
                        disabled={pendingProcess === process.name}
                        onClick={() => runProcessAction(process.name, () => client.stopProcess(process.name))}
                      >
                        <Pause aria-hidden="true" size={15} />
                      </IconButton>
                      <IconButton
                        label="재시작"
                        disabled={pendingProcess === process.name}
                        onClick={() => runProcessAction(process.name, () => client.restartProcess(process.name))}
                      >
                        <RotateCcw aria-hidden="true" size={15} />
                      </IconButton>
                      <IconButton
                        label={
                          process.managementMode.type === "direct"
                            ? "SystemRegistered로 전환"
                            : "Direct로 전환"
                        }
                        disabled={pendingProcess === process.name}
                        onClick={() => handleConvert(process.name, process.managementMode.type)}
                      >
                        <ArrowRightLeft aria-hidden="true" size={15} />
                      </IconButton>
                      <IconButton
                        label="삭제"
                        disabled={pendingProcess === process.name}
                        onClick={() => runProcessAction(process.name, () => client.removeProcess(process.name, true))}
                      >
                        <Trash2 aria-hidden="true" size={15} />
                      </IconButton>
                    </div>
                  </TableCell>
                </tr>
              ))}
            </DataTable>
          )}
        </Panel>
      </div>

      <aside className="grid content-start gap-5">
        <Panel>
          <PanelHeader title="추가·수정 폼" description="실제 저장 대신 화면 구조만 표현합니다." />
          <div className="grid gap-3 p-4">
            <Field label="이름">
              <input
                className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                onChange={(event) => setFormName(event.target.value)}
                value={formName}
              />
            </Field>
            <Field label="명령">
              <input
                className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                onChange={(event) => setFormCommand(event.target.value)}
                value={formCommand}
              />
            </Field>
            <Field label="작업 경로">
              <input
                className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                onChange={(event) => setFormCwd(event.target.value)}
                value={formCwd}
              />
            </Field>
            <fieldset className="grid gap-2 rounded-lg border border-border p-3">
              <legend className="px-1 text-xs font-medium text-muted">관리 모드</legend>
              <label className="flex items-center gap-2 text-sm text-foreground">
                <input
                  checked={selectedManagementMode === "direct"}
                  name="management-mode"
                  onChange={() => setSelectedManagementMode("direct")}
                  type="radio"
                />
                Direct
              </label>
              <label className="flex items-center gap-2 text-sm text-foreground">
                <input
                  checked={selectedManagementMode === "system_registered"}
                  name="management-mode"
                  onChange={() => setSelectedManagementMode("system_registered")}
                  type="radio"
                />
                SystemRegistered
              </label>
            </fieldset>
            {selectedManagementMode === "system_registered" ? (
              <div className="grid gap-3 rounded-lg border border-info/30 bg-info/10 p-3">
                <div>
                  <p className="text-xs font-semibold text-info">실제 시스템 등록 미리보기</p>
                  <p className="mt-1 text-xs text-muted">
                    현재 OS에 맞는 서비스 등록 명령과 설정 파일 내용을 복사할 수 있습니다.
                  </p>
                </div>
                <div className="rounded-md border border-border bg-surface px-3 py-2">
                  <p className="text-xs font-medium text-muted">감지된 실행 환경</p>
                  <p className="mt-1 text-sm font-semibold text-foreground">
                    {servicePlatformLabels[servicePlatform]}
                  </p>
                </div>
                <div className="grid gap-2">
                  <div className="flex items-center justify-between gap-3">
                    <div className="min-w-0">
                      <p className="text-xs font-semibold text-foreground">
                        {serviceRegistrationPreview.commandTitle}
                      </p>
                      <p className="mt-1 text-xs text-muted">터미널에서 실행할 명령입니다.</p>
                    </div>
                    <Button
                      className="shrink-0"
                      onClick={() => copySnippet("command", serviceRegistrationPreview.command)}
                    >
                      {copiedSnippet === "command" ? (
                        <Check aria-hidden="true" size={16} />
                      ) : (
                        <Copy aria-hidden="true" size={16} />
                      )}
                      {copiedSnippet === "command" ? "복사됨" : "복사"}
                    </Button>
                  </div>
                  <pre className="max-w-full overflow-x-auto whitespace-pre-wrap rounded-md border border-border bg-background p-3 font-mono text-xs text-foreground">
                    <code>{serviceRegistrationPreview.command}</code>
                  </pre>
                </div>
                <div className="grid gap-2">
                  <div className="flex items-center justify-between gap-3">
                    <div className="min-w-0">
                      <p className="text-xs font-semibold text-foreground">설정 파일</p>
                      <p className="mt-1 break-all font-mono text-xs text-muted">
                        {serviceRegistrationPreview.configPath}
                      </p>
                    </div>
                    <Button
                      className="shrink-0"
                      onClick={() => copySnippet("config", serviceRegistrationPreview.config)}
                    >
                      {copiedSnippet === "config" ? (
                        <Check aria-hidden="true" size={16} />
                      ) : (
                        <Copy aria-hidden="true" size={16} />
                      )}
                      {copiedSnippet === "config" ? "복사됨" : "복사"}
                    </Button>
                  </div>
                  <pre className="max-h-80 max-w-full overflow-auto rounded-md border border-border bg-background p-3 font-mono text-xs text-foreground">
                    <code>{serviceRegistrationPreview.config}</code>
                  </pre>
                </div>
              </div>
            ) : null}
            <div className="grid grid-cols-2 gap-2">
              <Button variant="primary" disabled={pendingProcess !== null} onClick={handleAddProcess}>
                저장
              </Button>
              <Button>취소</Button>
            </div>
          </div>
        </Panel>

        <Panel>
          <PanelHeader title="관리 모드 전환" description="Direct ↔ SystemRegistered(launchd) 실제 전환 결과를 표시합니다." />
          <div className="grid gap-3 p-4 text-sm">
            {actionError ? (
              <div className="rounded-md border border-danger/30 bg-danger/10 p-3 text-danger">
                {actionError}
              </div>
            ) : (
              <p className="text-xs text-muted">
                각 프로세스 행의 전환 버튼(<ArrowRightLeft className="inline align-text-bottom" aria-hidden="true" size={13} />)으로
                Direct ↔ SystemRegistered를 전환합니다. 실패 시 데몬이 원래 모드로 롤백하며 그 결과가 여기에 표시됩니다.
              </p>
            )}
            <p className="text-xs text-muted">
              SystemRegistered 전환은 macOS에서 launchd LaunchAgent를 생성·부트스트랩합니다. 적용될 plist는 위 폼의 미리보기에서 확인할 수 있습니다.
            </p>
          </div>
        </Panel>
      </aside>
    </div>
  );
}
