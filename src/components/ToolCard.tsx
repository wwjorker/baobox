import { useI18n } from "../i18n";
import { ToolIcon, pillarOf } from "../tools/icons";
import type { ToolDef } from "../tools/registry";

/**
 * 工具卡片：分类色图标块 + 名字/说明 + 收藏星。
 * 支柱页的网格和首页的「常用」共用这一个，保证观感一致。
 */
export function ToolCard({
  tool,
  fav,
  onFav,
  onOpen,
}: {
  tool: ToolDef;
  fav: boolean;
  onFav: () => void;
  onOpen: () => void;
}) {
  const { t } = useI18n();
  return (
    <div className="cardwrap">
      <button className={`card card--${pillarOf(tool.id)}`} onClick={onOpen}>
        <span className="card__ico">
          <ToolIcon id={tool.id} />
        </span>
        <span className="card__body">
          <span className="card__name">
            {t(`tool.${tool.id}.name` as never)}
            {tool.highlight && <span className="badge is-highlight">{t("status.highlight")}</span>}
            {tool.status !== "ready" && (
              <span className="badge">{t(`status.${tool.status}` as never)}</span>
            )}
          </span>
          <span className="card__desc">{t(`tool.${tool.id}.desc` as never)}</span>
        </span>
      </button>
      {/* 星标是独立按钮、不嵌在卡片按钮里（按钮不能套按钮）。 */}
      <button
        className={`cardstar${fav ? " is-on" : ""}`}
        title={fav ? t("fav.remove") : t("fav.add")}
        aria-pressed={fav}
        onClick={onFav}
      >
        {fav ? "★" : "☆"}
      </button>
    </div>
  );
}
