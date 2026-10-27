import {
  Activity,
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
import { lazy,Suspense,useCallback, useEffect, useRef, useState } from "react";
import { DaemonView } from "./features/daemon/DaemonView";
const JobsView=lazy(()=>import("./features/jobs/JobsView").then(module=>({default:module.JobsView})));
import { LogsView } from "./features/logs/LogsView";
import { ProcessesView } from "./features/processes/ProcessesView";
import { SettingsView } from "./features/settings/SettingsView";
import { useOperationsClient, useDaemonResource, useProcessResource, refreshResources } from "./services/use-operations";
import {EditorLeavePrompt,requestEditorLeave} from "./shared/editor-session";
import type { NavigationKey, ThemePreference } from "./shared/types";

const DAEMON_STATUS_POLL_INTERVAL_MS = 2000;

const navigationItems: Array<{
  key: NavigationKey;
  label: string;
  icon: typeof Activity;
}> = [
  { key: "processes", label: "프로세스", icon: Activity },
  { key: "jobs", label: "작업", icon: BriefcaseBusiness },
  { key: "logs", label: "로그", icon: TerminalSquare },
  { key: "daemon", label: "데몬", icon: MonitorCog },
  { key: "settings", label: "설정", icon: Settings },
];

const themeLabels: Record<ThemePreference, string> = {
  auto: "시스템",
  dark: "다크",
  light: "라이트",
};

function resolveSystemTheme() {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function App() {
  const client = useOperationsClient();
  const [activeNavigationKey, setActiveNavigationKey] = useState<NavigationKey>(() => {
    const page = new URLSearchParams(window.location.search).get("page");
    return navigationItems.some(item => item.key === page) ? page as NavigationKey : "processes";
  });
  const [themePreference, setThemePreference] = useState<ThemePreference>(() => {
    const value = localStorage.getItem("theme");
    return value === "dark" || value === "light" ? value : "auto";
  });

  const [routeKey,setRouteKey]=useState(location.search);
  const previousUrl=useRef(location.href);
  useEffect(() => {
    const applyUrl=(url:URL)=>{const page=url.searchParams.get("page");setActiveNavigationKey(navigationItems.some(item=>item.key===page)?page as NavigationKey:"processes");setRouteKey(url.search);previousUrl.current=url.href;};
    const onLink=(event:MouseEvent)=>{
      if(event.defaultPrevented||event.button!==0||event.metaKey||event.ctrlKey||event.shiftKey||event.altKey)return;
      const link=(event.target as Element).closest<HTMLAnchorElement>("a[href]");if(!link||link.download||link.target==="_blank"||link.getAttribute("href")?.startsWith("#"))return;
      const url=new URL(link.href,location.href);if(url.origin!==location.origin||!url.searchParams.has("page"))return;
      event.preventDefault();requestEditorLeave(()=>{history.pushState({},"",url);applyUrl(url);});
    };
    const onPopState = () => {
      const target=new URL(location.href);const restore=previousUrl.current;const previous=new URL(restore);if(target.pathname===previous.pathname&&target.search===previous.search){previousUrl.current=target.href;return;}requestEditorLeave(()=>applyUrl(target),undefined,()=>history.pushState({},"",restore));
    };
    const recordLocation=()=>{previousUrl.current=location.href;};
    window.addEventListener("msv:editor-location",recordLocation);window.addEventListener("popstate", onPopState);document.addEventListener("click",onLink);
    return () => {window.removeEventListener("msv:editor-location",recordLocation);window.removeEventListener("popstate", onPopState);document.removeEventListener("click",onLink);};
  }, []);

  function navigate(page: NavigationKey) {
    if(page===activeNavigationKey)return;
    requestEditorLeave(()=>{setActiveNavigationKey(page);
      const url = new URL(window.location.href); url.search = ""; url.searchParams.set("page", page);
      window.history.pushState({}, "", url);previousUrl.current=url.href;setRouteKey(url.search);});
  }

  const daemonResource=useDaemonResource();const processResource=useProcessResource();
  const {data:daemonStatus,errorMessage:daemonError}=daemonResource;
  const processes=processResource.data;
  const [isRefreshing, setIsRefreshing] = useState(false);

  const [refreshNotice,setRefreshNotice]=useState("");
  const isDaemonConnected = daemonStatus !== null && daemonError === null;
  const processList = processes ?? [];
  const runningProcessCount = processList.filter((process) => process.state === "running").length;

  const handleRefresh = useCallback(async () => {
    setIsRefreshing(true);
    try {
      const results=await refreshResources(client);const failed=results.filter(item=>item.result==="error");setRefreshNotice(failed.length?`일부 데이터 ${failed.length}개를 갱신하지 못했습니다.`:"현재 화면의 데이터를 새로고침했습니다.");
    } finally {
      setIsRefreshing(false);
    }
  }, [client]);

  useEffect(() => {
    localStorage.setItem("theme", themePreference);
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
      <EditorLeavePrompt/>
      <a
        className="sr-only focus:not-sr-only focus:fixed focus:left-4 focus:top-4 focus:z-50 focus:rounded-md focus:bg-primary focus:px-3 focus:py-2 focus:text-sm focus:font-semibold focus:text-white"
        href="#main-content"
      >
        본문으로 이동
      </a>
      <div className="grid min-h-screen grid-cols-1 lg:grid-cols-[188px_minmax(0,1fr)]">
        <aside className="app-sidebar min-w-0 border-b border-border bg-surface lg:border-b-0 lg:border-r">
          <div className="flex h-full flex-col">
            <div className="flex h-16 items-center gap-3 border-b border-border px-4">
              <div className="flex h-9 w-9 items-center justify-center rounded-md bg-[rgb(var(--color-action))] text-white">
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
                        ? "bg-primary/10 text-primary"
                        : "text-muted hover:bg-panel hover:text-foreground"
                    }`}
                    key={item.key}
                    onClick={() => navigate(item.key)}
                    type="button"
                  >
                    <Icon aria-hidden="true" size={18} />
                    <span>{item.label}</span>
                  </button>
                );
              })}
            </nav>
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
                  {isDaemonConnected ? "로컬 데몬" : daemonResource.isLoading ? "상태 확인 중" : "상태 확인 실패"}
                  <span className="font-mono">PID {daemonStatus?.pid ?? "—"}</span>
                </div>

              </div>
              <div className="flex flex-wrap items-center gap-2">
                <div className="hidden items-center gap-2 rounded-md border border-border bg-panel px-3 py-2 text-xs text-muted sm:flex">
                  <ListTree aria-hidden="true" size={15} />
                  {processes===null?"—":runningProcessCount}/{processes===null?"—":`${processList.length}${processResource.hasMore?"+":""}`} 프로세스 실행 중{processResource.isStale?" · 이전 값":""}
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
            {refreshNotice&&<p role="status" className="editor-note">{refreshNotice}</p>}
            <Suspense fallback={<div className="editor-loading">작업 화면을 불러오는 중…</div>}><div key={routeKey}>{content}</div></Suspense>
          </main>
        </div>
      </div>
    </div>
  );
}

export default App;
