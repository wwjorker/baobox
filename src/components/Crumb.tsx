import { useI18n } from "../i18n";
import type { Pillar } from "../tools/registry";

/**
 * 工具页顶部的面包屑。
 *
 * 原先五个面板各写了一份，且只是静态文字——长得像导航却点不动，
 * 想退回去只能去点侧栏。这里统一成一个能真的返回的按钮，
 * 并把 Esc 也标出来，免得那个快捷键没人知道。
 */
export function Crumb({
  pillar,
  name,
  onBack,
}: {
  pillar: Pillar;
  name: string;
  onBack: () => void;
}) {
  const { t } = useI18n();
  return (
    <div className="crumb">
      <button className="crumb__back" onClick={onBack}>
        <span className="crumb__arrow">‹</span>
        {t(`pillar.${pillar}` as never)}
      </button>
      <span>›</span>
      <b>{name}</b>
      <span className="crumb__esc">Esc</span>
    </div>
  );
}
