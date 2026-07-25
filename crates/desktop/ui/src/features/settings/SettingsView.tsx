import { Monitor, Moon, Save, Sun } from "lucide-react";
import { Button, Field, Panel, PanelHeader } from "../../components/ui/primitives";
import type { ThemePreference } from "../../shared/types";

const themeOptions: Array<{ value: ThemePreference; label: string; icon: typeof Monitor }> = [
  { value: "auto", label: "자동", icon: Monitor },
  { value: "dark", label: "다크", icon: Moon },
  { value: "light", label: "라이트", icon: Sun },
];

export function SettingsView({
  themePreference,
  onThemePreferenceChange,
}: {
  themePreference: ThemePreference;
  onThemePreferenceChange: (themePreference: ThemePreference) => void;
}) {
  return (
    <div className="grid gap-5 xl:grid-cols-[minmax(0,1fr)_360px]">
      <Panel>
        <PanelHeader description="Tauri와 외부 브라우저에서 공통으로 쓰는 UI 설정입니다." title="설정" />
        <div className="grid gap-5 p-4">
          <div className="grid gap-3 md:grid-cols-2">
            <Field label="API 기준 주소" value="http://127.0.0.1:9876" />
            <Field label="로그 tail 기본 줄 수" value="100" />
          </div>

          <fieldset className="grid gap-3 rounded-lg border border-border p-4">
            <legend className="px-1 text-sm font-semibold text-foreground">테마</legend>
            <div className="grid gap-2 sm:grid-cols-3">
              {themeOptions.map((option) => {
                const Icon = option.icon;
                const isActive = option.value === themePreference;
                return (
                  <button
                    className={`flex h-20 items-center justify-center gap-2 rounded-md border text-sm font-medium transition-colors duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary ${
                      isActive
                        ? "border-primary bg-primary text-white"
                        : "border-border bg-surface text-muted hover:bg-background hover:text-foreground"
                    }`}
                    key={option.value}
                    onClick={() => onThemePreferenceChange(option.value)}
                    type="button"
                  >
                    <Icon aria-hidden="true" size={18} />
                    {option.label}
                  </button>
                );
              })}
            </div>
          </fieldset>

          <fieldset className="grid gap-3 rounded-lg border border-border p-4">
            <legend className="px-1 text-sm font-semibold text-foreground">데스크톱</legend>
            <label className="flex items-start gap-3 text-sm text-foreground">
              <input className="mt-1" disabled type="checkbox" />
              <span>
                로그인 시 데몬 자동 시작
                <span className="mt-1 block text-xs text-muted">
                  아직 지원하지 않아 변경할 수 없습니다.
                </span>
              </span>
            </label>
            <label className="flex items-start gap-3 text-sm text-foreground">
              <input className="mt-1" disabled type="checkbox" />
              <span>
                충돌 감지 시 네이티브 알림 표시
                <span className="mt-1 block text-xs text-muted">
                  아직 지원하지 않아 변경할 수 없습니다.
                </span>
              </span>
            </label>
          </fieldset>
        </div>
      </Panel>

      <aside className="grid content-start gap-5">
        <Panel>
          <PanelHeader title="변경 사항" description="테마는 즉시 적용되며 데몬 설정 저장은 아직 지원하지 않습니다." />
          <div className="grid gap-3 p-4">
            <Button disabled variant="primary">
              <Save aria-hidden="true" size={16} />
              데몬 설정 저장 미지원
            </Button>
            <p className="text-xs text-muted">
              지원되지 않는 설정은 성공한 것처럼 표시하지 않습니다.
            </p>
          </div>
        </Panel>
      </aside>
    </div>
  );
}
