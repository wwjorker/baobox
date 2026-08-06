import type { ReactNode } from "react";
import { useI18n } from "../i18n";
import { ToolIcon, pillarOf } from "../tools/icons";

/**
 * 工具页标题：分类色图标块 + 工具名（+ 可选徽标）。
 * 让每个工具在卡片、侧栏、命令面板、工具页里都露出同一个图标,一眼认得出。
 */
export function ToolHead({ id, children }: { id: string; children?: ReactNode }) {
  const { t } = useI18n();
  return (
    <div className="toolhead">
      <span className={`toolhead__ico toolhead__ico--${pillarOf(id)}`}>
        <ToolIcon id={id} />
      </span>
      <h1 className="h1">
        {t(`tool.${id}.name` as never)}
        {children}
      </h1>
    </div>
  );
}
