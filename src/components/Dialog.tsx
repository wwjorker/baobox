import { useEffect, useId, useRef, type ReactNode } from "react";
import { useI18n } from "../i18n";

const FOCUSABLE =
  'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])';

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
  aside,
  onClose,
}: {
  title: string;
  children: ReactNode;
  /**
   * 放在按钮行左端的次要操作。
   *
   * 必须走这里而不是塞进 children —— 弹层是纵向 flex，直接放进去会被撑成通栏，
   * 「已省下」那个归零按钮就横成了一条大红，比「知道了」还抢眼，
   * 而它是个破坏性操作，不该是弹层里最显眼的东西。
   */
  aside?: ReactNode;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const boxRef = useRef<HTMLDivElement>(null);
  const titleId = useId();

  // 焦点管理：打开时把焦点移进弹层，Tab 在里面循环，关闭后还给原来的元素。
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    const box = boxRef.current;
    if (box) (box.querySelector<HTMLElement>(FOCUSABLE) ?? box).focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Tab" || !box) return;
      const items = Array.from(box.querySelectorAll<HTMLElement>(FOCUSABLE));
      if (items.length === 0) return;
      const first = items[0];
      const last = items[items.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
      prev?.focus?.();
    };
  }, []);

  return (
    <div className="confirm" onMouseDown={onClose}>
      <div
        className="confirm__box is-info"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        ref={boxRef}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <h2 className="confirm__title" id={titleId}>{title}</h2>
        {children}
        <div className="confirm__actions is-end">
          {aside}
          <span className="confirm__gap" />
          <button className="chip is-primary" onClick={onClose}>
            {t("app.gotIt")}
          </button>
        </div>
      </div>
    </div>
  );
}
