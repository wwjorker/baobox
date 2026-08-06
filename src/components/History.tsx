import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useI18n } from "../i18n";
import { ToolIcon, pillarOf } from "../tools/icons";
import type { HistEntry } from "../useHistory";

/** 相对时间：刚刚 / N 分钟前 / 今天 HH:MM / 昨天 HH:MM / 更早的日期。 */
function groupOf(time: number, t: (k: string) => string): string {
  const now = new Date();
  const d = new Date(time);
  const sameDay = now.toDateString() === d.toDateString();
  const y = new Date(now.getTime() - 86400000);
  const yDay = y.toDateString() === d.toDateString();
  if (sameDay) return t("history.today");
  if (yDay) return t("history.yesterday");
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function clock(time: number): string {
  const d = new Date(time);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

export function History({
  history,
  onClear,
  onOpenTool,
}: {
  history: HistEntry[];
  onClear: () => void;
  onOpenTool: (id: string) => void;
}) {
  const { t } = useI18n();

  // 按天分组，保持时间倒序
  const groups: { label: string; items: HistEntry[] }[] = [];
  for (const e of history) {
    const label = groupOf(e.time, t as never);
    const last = groups[groups.length - 1];
    if (last && last.label === label) last.items.push(e);
    else groups.push({ label, items: [e] });
  }

  return (
    <div className="toolpage">
      <div className="histhead">
        <h1 className="h1">{t("history.title")}</h1>
        <span className="hist__note">
          <span className="hist__dot" />
          {t("history.note")}
        </span>
        {history.length > 0 && (
          <button className="chip histhead__clear" onClick={onClear}>
            {t("history.clear")}
          </button>
        )}
      </div>

      {history.length === 0 ? (
        <div className="empty">
          <div className="empty__box">◷</div>
          <h2 className="empty__title">{t("history.emptyTitle")}</h2>
          <p className="empty__hint">{t("history.emptyHint")}</p>
        </div>
      ) : (
        <div className="histlist">
          {groups.map((g) => (
            <div key={g.label} className="histgroup">
              <div className="histday">{g.label}</div>
              {g.items.map((e) => (
                <div key={e.id} className="histrow">
                  <span className={`histrow__ico histrow__ico--${pillarOf(e.toolId)}`}>
                    <ToolIcon id={e.toolId} />
                  </span>
                  <div className="histrow__main">
                    <div className="histrow__t">
                      {t(`tool.${e.toolId}.name` as never)}
                      <span className="histrow__meta">{e.summary}</span>
                    </div>
                    {e.outPath && (
                      <div className="histrow__loc" title={e.outPath}>
                        {e.outPath}
                      </div>
                    )}
                  </div>
                  <span className="histrow__time">{clock(e.time)}</span>
                  <div className="histrow__acts">
                    {e.outPath && (
                      <button className="chip" onClick={() => revealItemInDir(e.outPath!)}>
                        {t("run.openOutput")}
                      </button>
                    )}
                    <button className="chip" onClick={() => onOpenTool(e.toolId)}>
                      {t("history.again")}
                    </button>
                  </div>
                </div>
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
