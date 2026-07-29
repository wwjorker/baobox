import type { ReactNode } from "react";
import { useI18n } from "../i18n";

/**
 * 说明性弹层。
 *
 * 跟删除确认那种弹层刻意分开：确认框的主按钮是「取消」，做成整条大红是对的
 * ——它要挡住手滑。而一个只是解释一件事的弹层，把「知道了」也做成同样的
 * 整条大红，等于在喊，看着像出了事。这里只给一个正常大小的按钮。
 */
export function Dialog({
  title,
  children,
  onClose,
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
}) {
  const { t } = useI18n();
  return (
    <div className="confirm" onMouseDown={onClose}>
      <div
        className="confirm__box is-info"
        role="dialog"
        aria-modal="true"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <h2 className="confirm__title">{title}</h2>
        {children}
        <div className="confirm__actions is-end">
          <button className="chip is-primary" onClick={onClose}>
            {t("app.gotIt")}
          </button>
        </div>
      </div>
    </div>
  );
}
