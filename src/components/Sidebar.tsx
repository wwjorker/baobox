import { useI18n } from "../i18n";
import { PILLARS, toolsOf, type Pillar } from "../tools/registry";
import { ToolIcon, pillarOf } from "../tools/icons";

interface Props {
  active: Pillar;
  /** 当前是否在仪表盘首页 */
  home: boolean;
  recent: string[];
  favorites: string[];
  onHome: () => void;
  onPillar: (p: Pillar) => void;
  onTool: (id: string) => void;
}

export function Sidebar({ active, home, recent, favorites, onHome, onPillar, onTool }: Props) {
  const { t } = useI18n();

  return (
    <nav className="sidebar">
      <button className="nav nav--home" aria-current={home} onClick={onHome}>
        <span className="nav__ico nav__ico--home">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M4 11l8-7 8 7" />
            <path d="M6 10v9a1 1 0 0 0 1 1h10a1 1 0 0 0 1-1v-9" />
          </svg>
        </span>
        {t("pillar.home")}
      </button>

      <div className="sidebar__label">{t("pillar.tools")}</div>

      {PILLARS.map((p) => {
        const all = toolsOf(p);
        const ready = all.filter((x) => x.status === "ready").length;
        // 计数原先直接用总数，PDF 显示 10 但其中一个是「暂未实现」，
        // 点进去才发现，等于数字在虚报。没到齐就把两个数都摆出来。
        const full = ready === all.length;
        return (
          <button
            key={p}
            className="nav nav--pillar"
            aria-current={!home && p === active}
            title={full ? undefined : t("pillar.readyCount", { ready, total: all.length })}
            onClick={() => onPillar(p)}
          >
            <span className={`nav__ico nav__ico--${p}`}>
              <ToolIcon id={p} />
            </span>
            {t(`pillar.${p}` as never)}
            <span className={`nav__count${full ? "" : " is-partial"}`}>
              {full ? all.length : `${ready}/${all.length}`}
            </span>
          </button>
        );
      })}

      {/* 收藏跨支柱，一点直达。放在「最近」之前——钉住的是主动选的常用，
          比自动滚动的最近更该靠上。 */}
      {favorites.length > 0 && (
        <>
          <div className="sidebar__label">{t("fav.section")}</div>
          {favorites.map((id) => (
            <button key={id} className="nav nav--sm" onClick={() => onTool(id)}>
              <span className={`nav__mini nav__mini--${pillarOf(id)}`}>
                <ToolIcon id={id} />
              </span>
              {t(`tool.${id}.name` as never)}
            </button>
          ))}
        </>
      )}

      {recent.length > 0 && (
        <>
          <div className="sidebar__label">{t("pillar.recent")}</div>
          {recent.map((id) => (
            <button key={id} className="nav nav--sm" onClick={() => onTool(id)}>
              <span className={`nav__mini nav__mini--${pillarOf(id)}`}>
                <ToolIcon id={id} />
              </span>
              {t(`tool.${id}.name` as never)}
            </button>
          ))}
        </>
      )}
    </nav>
  );
}
