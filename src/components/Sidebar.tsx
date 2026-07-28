import { useI18n } from "../i18n";
import { PILLARS, PILLAR_GLYPH, toolsOf, type Pillar } from "../tools/registry";

interface Props {
  active: Pillar;
  recent: string[];
  onPillar: (p: Pillar) => void;
  onTool: (id: string) => void;
}

export function Sidebar({ active, recent, onPillar, onTool }: Props) {
  const { t } = useI18n();

  return (
    <nav className="sidebar">
      <div className="sidebar__label">{t("pillar.tools")}</div>

      {PILLARS.map((p) => (
        <button
          key={p}
          className="nav"
          aria-current={p === active}
          onClick={() => onPillar(p)}
        >
          <span className="nav__glyph">{PILLAR_GLYPH[p]}</span>
          {t(`pillar.${p}` as never)}
          <span className="nav__count">{toolsOf(p).length}</span>
        </button>
      ))}

      {recent.length > 0 && (
        <>
          <div className="sidebar__label">{t("pillar.recent")}</div>
          {recent.map((id) => (
            <button key={id} className="nav" onClick={() => onTool(id)}>
              <span className="nav__glyph">·</span>
              {t(`tool.${id}.name` as never)}
            </button>
          ))}
        </>
      )}
    </nav>
  );
}
