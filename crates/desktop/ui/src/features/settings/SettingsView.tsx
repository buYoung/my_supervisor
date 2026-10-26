import {Monitor,Moon,Sun,ArrowUpRight} from "lucide-react";
import {useCallback} from "react";
import {useOperationsClient,usePolledResource} from "../../services/use-operations";
import type {ThemePreference} from "../../shared/types";
export function SettingsView({themePreference,onThemePreferenceChange}:{themePreference:ThemePreference;onThemePreferenceChange:(value:ThemePreference)=>void}){
  const client=useOperationsClient();const status=usePolledResource(useCallback(()=>client.daemonStatus(),[client]),10000);
  return <div className="settings-workspace"><header className="workspace-heading"><div><h1>설정</h1><p>화면 모양과 데스크톱 실행 정보를 확인합니다.</p></div></header><div className="workspace-panel settings-list">
    <section><header><h2>화면 모양</h2><p>변경하면 바로 적용됩니다.</p></header><div className="theme-choices">{([{value:"light",label:"라이트",Icon:Sun},{value:"dark",label:"다크",Icon:Moon},{value:"auto",label:"시스템",Icon:Monitor}] as const).map(({value,label,Icon})=><button className={themePreference===value?"selected":""} key={value} aria-pressed={themePreference===value} onClick={()=>onThemePreferenceChange(value)}><div className={`theme-preview theme-${value}`}><span/><span/><span/></div><span><Icon size={14}/>{label}</span></button>)}<p className="editor-note">시스템을 선택하면 운영체제의 화면 모양을 따릅니다.</p></div></section>
    <section><header><h2>실행과 창</h2><p>프로세스의 실행 수명</p></header><div><p className="text-sm leading-7">프로세스별로 앱과 함께 종료할지, 앱 종료 후에도 유지할지 선택할 수 있습니다.</p><p className="editor-note">프로세스 → 설정 수정 → 실행 수명에서 변경하세요. 데스크톱 창을 닫아도 메뉴 막대에 남아 실행을 유지합니다. 메뉴 막대에서 앱을 종료할 수 있습니다.</p></div></section>
    <section><header><h2>이 앱에 관하여</h2><p>현재 연결과 실행 정보</p></header><dl className="detail-values"><dt>my-supervisor</dt><dd>{status.data?.version||"—"}</dd><dt>연결 방식</dt><dd>{client.transport==="invoke"?"데스크톱 로컬 런타임":"로컬 HTTP 연결"}</dd><dt>데몬 PID</dt><dd>{status.data?.pid||"—"}</dd><dt>연결 상태</dt><dd><a href="?page=daemon" className="inline-flex items-center gap-1 text-primary">데몬 상태 확인<ArrowUpRight size={13}/></a></dd></dl></section>
  </div></div>;
}
