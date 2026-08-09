import { useEffect, useRef, type RefObject } from "react";

const FOCUSABLE =
  'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])';

/**
 * 弹层的键盘可达性。
 *
 * 打开时把焦点移进弹层，Tab / Shift+Tab 在里面循环（不会跑到背景界面），
 * Esc 关闭，关闭后把焦点还给打开前那个元素。
 *
 * 初始焦点默认落在第一个可聚焦元素；给某个元素加 `data-autofocus` 可以
 * 指定落点——删除 / 粉碎确认就靠它把默认焦点钉在「取消」上（安全红线 4），
 * 而不是 DOM 里第一个可聚焦元素（在粉碎框里那是输入框）。
 *
 * `active` 用于随条件渲染的弹层开合：从 false→true 时聚焦并接管键盘，
 * true→false 时移除监听、还原焦点。所以内联在面板里的确认框也能用，
 * 不必单独抽成组件。
 */
export function useFocusTrap(
  boxRef: RefObject<HTMLElement | null>,
  active: boolean,
  onClose: () => void,
) {
  // 用 ref 存最新的 onClose，effect 只依赖 active，不因每次渲染的新闭包重跑
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useEffect(() => {
    if (!active) return;
    const prev = document.activeElement as HTMLElement | null;
    const box = boxRef.current;
    if (box) {
      const initial =
        box.querySelector<HTMLElement>("[data-autofocus]") ??
        box.querySelector<HTMLElement>(FOCUSABLE) ??
        box;
      initial.focus();
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        closeRef.current();
        return;
      }
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
  }, [active, boxRef]);
}
