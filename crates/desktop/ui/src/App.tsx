import {
  Activity,
  Bell,
  BriefcaseBusiness,
  CheckCircle2,
  CircleDot,
  HardDrive,
  ListTree,
  MonitorCog,
  Moon,
  RefreshCw,
  Settings,
  Sun,
  TerminalSquare,
  XCircle,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { DaemonView } from "./features/daemon/DaemonView";
import { JobsView } from "./features/jobs/JobsView";
import { LogsView } from "./features/logs/LogsView";
import { ProcessesView } from "./features/processes/ProcessesView";
import { SettingsView } from "./features/settings/SettingsView";
import { useOperationsClient, usePolledResource } from "./services/use-operations";
import type { NavigationKey, ThemePreference } from "./shared/types";

const DAEMON_STATUS_POLL_INTERVAL_MS = 2000;

const navigationItems: Array<{
  key: NavigationKey;
  label: string;
  icon: typeof Activity;
}> = [
  { key: "processes", label: "Processes", icon: Activity },
  { key: "jobs", label: "Jobs", icon: BriefcaseBusiness },
  { key: "logs", label: "Logs", icon: TerminalSquare },
  { key: "daemon", label: "Daemon", icon: MonitorCog },
  { key: "settings", label: "Settings", icon: Settings },
];

const themeLabels: Record<ThemePreference, string> = {
  auto: "자동",
  dark: "다크",
  light: "라이트",
};

function resolveSystemTheme() {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function App() {
  const client = useOperationsClient();
  const [activeNavigationKey, setActiveNavigationKey] = useState<NavigationKey>("processes");
  const [themePreference, setThemePreference] = useState<ThemePreference>("auto");

  const fetchDaemonStatus = useCallback(() => client.daemonStatus(), [client]);
  const {
    data: daemonStatus,
    errorMessage: daemonError,
    refresh: refreshDaemonStatus,
  } = usePolledResource(
    fetchDaemonStatus,
    DAEMON_STATUS_POLL_INTERVAL_MS,
  );
  const fetchProcesses = useCallback(() => client.listProcesses(), [client]);
  const { data: processes, refresh: refreshProcesses } = usePolledResource(
    fetchProcesses,
    DAEMON_STATUS_POLL_INTERVAL_MS,
  );
  const [isRefreshing, setIsRefreshing] = useState(false);

  const isDaemonConnected = daemonStatus !== null && daemonError === null;
  const processList = processes ?? [];
  const runningProcessCount = processList.filter((process) => process.state === "running").length;

  const handleRefresh = useCallback(async () => {
    setIsRefreshing(true);
    try {
      await Promise.all([refreshDaemonStatus(), refreshProcesses()]);
    } finally {
      setIsRefreshing(false);
    }
  }, [refreshDaemonStatus, refreshProcesses]);

  useEffect(() => {
    const rootElement = document.documentElement;
    const applyTheme = () => {
      const resolvedTheme = themePreference === "auto" ? resolveSystemTheme() : themePreference;
      rootElement.dataset.theme = resolvedTheme;
      rootElement.style.colorScheme = resolvedTheme;
    };

    applyTheme();

    if (themePreference !== "auto") {
      return;
    }

    const mediaQueryList = window.matchMedia("(prefers-color-scheme: dark)");
    mediaQueryList.addEventListener("change", applyTheme);
    return () => mediaQueryList.removeEventListener("change", applyTheme);
  }, [themePreference]);

  const content = {
    processes: <ProcessesView />,
    jobs: <JobsView />,
    logs: <LogsView />,
    daemon: <DaemonView />,
    settings: (
      <SettingsView themePreference={themePreference} onThemePreferenceChange={setThemePreference} />
    ),
  }[activeNavigationKey];

  return (
    <div className="min-h-screen bg-background">
      <a
        className="sr-only focus:not-sr-only focus:fixed focus:left-4 focus:top-4 focus:z-50 focus:rounded-md focus:bg-primary focus:px-3 focus:py-2 focus:text-sm focus:font-semibold focus:text-white"
        href="#main-content"
      >
        본문으로 이동
      </a>
      <div className="grid min-h-screen grid-cols-1 lg:grid-cols-[248px_minmax(0,1fr)]">
        <aside className="min-w-0 border-b border-border bg-surface lg:border-b-0 lg:border-r">
          <div className="flex h-full flex-col">
            <div className="flex h-16 items-center gap-3 border-b border-border px-4">
              <div className="flex h-9 w-9 items-center justify-center rounded-md bg-primary text-white">
                <HardDrive aria-hidden="true" size={19} />
              </div>
              <div className="min-w-0">
                <p className="truncate text-sm font-semibold text-foreground">my-supervisor</p>
                <p className="truncate text-xs text-muted">로컬 프로세스 운영 콘솔</p>
              </div>
            </div>
            <nav aria-label="주 탐색" className="flex min-w-0 gap-1 overflow-x-auto px-3 py-3 lg:grid">
              {navigationItems.map((item) => {
                const Icon = item.icon;
                const isActive = item.key === activeNavigationKey;
                return (
                  <button
                    aria-current={isActive ? "page" : undefined}
                    className={`inline-flex h-10 shrink-0 items-center gap-3 rounded-md px-3 text-sm font-medium transition-colors duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary lg:w-full ${
                      isActive
                        ? "bg-primary text-white"
                        : "text-muted hover:bg-panel hover:text-foreground"
                    }`}
                    key={item.key}
                    onClick={() => setActiveNavigationKey(item.key)}
                    type="button"
                  >
                    <Icon aria-hidden="true" size={18} />
                    <span>{item.label}</span>
                  </button>
                );
              })}
            </nav>
            <div className="mt-auto hidden border-t border-border p-4 lg:block">
              <div className="rounded-lg border border-border bg-panel p-3">
                <div className="flex items-center gap-2 text-sm font-medium text-foreground">
                  {isDaemonConnected ? (
                    <CheckCircle2 aria-hidden="true" className="text-success" size={17} />
                  ) : (
                    <XCircle aria-hidden="true" className="text-danger" size={17} />
                  )}
                  {isDaemonConnected ? "데몬 연결됨" : "데몬 연결 안 됨"}
                </div>
                <p className="mt-2 font-mono text-xs text-muted">127.0.0.1:9876</p>
              </div>
            </div>
          </div>
        </aside>

        <div className="min-w-0">
          <header className="sticky top-0 z-30 border-b border-border bg-surface/95 px-4 backdrop-blur md:px-6">
            <div className="flex min-h-16 flex-wrap items-center justify-between gap-3 py-3">
              <div className="min-w-0">
                <div className="flex items-center gap-2 text-xs font-medium text-muted">
                  <CircleDot
                    aria-hidden="true"
                    className={isDaemonConnected ? "text-success" : "text-danger"}
                    size={14}
                  />
                  {isDaemonConnected ? "로컬 데몬" : "데몬 연결 끊김"}
                  <span className="font-mono">127.0.0.1:9876</span>
                </div>
                <h1 className="mt-1 truncate text-lg font-semibold text-foreground">
                  데스크톱 운영 콘솔
                </h1>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <div className="hidden items-center gap-2 rounded-md border border-border bg-panel px-3 py-2 text-xs text-muted sm:flex">
                  <ListTree aria-hidden="true" size={15} />
                  {runningProcessCount}/{processList.length} 실행 중
                </div>
                <button
                  className="inline-flex h-9 items-center gap-2 rounded-md border border-border bg-panel px-3 text-sm text-muted transition-colors duration-200 hover:bg-background hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:cursor-not-allowed disabled:opacity-60"
                  disabled={isRefreshing}
                  onClick={() => void handleRefresh()}
                  type="button"
                >
                  <RefreshCw aria-hidden="true" className={isRefreshing ? "animate-spin" : undefined} size={16} />
                  새로고침
                </button>
                <button
                  className="inline-flex h-9 cursor-not-allowed items-center gap-2 rounded-md border border-border bg-panel px-3 text-sm text-muted opacity-60"
                  disabled
                  title="이벤트 알림 UI는 아직 지원하지 않습니다."
                  type="button"
                >
                  <Bell aria-hidden="true" size={16} />
                  이벤트 미지원
                </button>
                <label className="inline-flex h-9 items-center gap-2 rounded-md border border-border bg-panel px-2 text-sm text-muted">
                  {themePreference === "dark" ? (
                    <Moon aria-hidden="true" size={16} />
                  ) : (
                    <Sun aria-hidden="true" size={16} />
                  )}
                  <span className="sr-only">테마</span>
                  <select
                    className="h-7 rounded bg-transparent text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                    onChange={(event) => setThemePreference(event.target.value as ThemePreference)}
                    value={themePreference}
                  >
                    {Object.entries(themeLabels).map(([value, label]) => (
                      <option key={value} value={value}>
                        {label}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
            </div>
          </header>
          <main className="px-4 py-5 md:px-6" id="main-content">
            {content}
          </main>
        </div>
      </div>
    </div>
  );
}

export default App;
