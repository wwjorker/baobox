import { useEffect, useRef } from "react";

/**
 * 歪箱子吉祥物 —— 百宝箱的门面。
 *
 * 眼睛跟着鼠标转、平时轻轻晃、鼠标移到拖入区会「开盖」准备接文件、
 * 接住/完成时蹦一下。它就是首页那个「接文件的箱子」。
 * 尊重「减少动态效果」：prefers-reduced-motion 下不追踪、不晃。
 */
export function Mascot({
  open,
  cheerKey,
  className,
}: {
  /** 开盖（拖入区悬停时张嘴接） */
  open?: boolean;
  /** 变化即触发一次庆祝跳动 */
  cheerKey?: number;
  className?: string;
}) {
  const ref = useRef<HTMLSpanElement>(null);
  const reduce =
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  // 瞳孔追踪鼠标
  useEffect(() => {
    if (reduce) return;
    const onMove = (e: PointerEvent) => {
      const el = ref.current;
      if (!el || !el.offsetParent) return;
      const b = el.getBoundingClientRect();
      const cx = b.left + b.width / 2;
      const cy = b.top + b.height / 2;
      const a = Math.atan2(e.clientY - cy, e.clientX - cx);
      const dx = (Math.cos(a) * 2.2).toFixed(2);
      const dy = (Math.sin(a) * 2.2).toFixed(2);
      el.querySelectorAll<SVGElement>(".m-pupil").forEach((p) =>
        p.setAttribute("transform", `translate(${dx},${dy})`),
      );
    };
    document.addEventListener("pointermove", onMove);
    return () => document.removeEventListener("pointermove", onMove);
  }, [reduce]);

  // 庆祝跳动：cheerKey 变化时重放一次动画
  useEffect(() => {
    if (reduce || cheerKey == null) return;
    const el = ref.current;
    if (!el) return;
    el.classList.remove("is-cheer");
    // 强制回流，动画才会重放
    void el.offsetWidth;
    el.classList.add("is-cheer");
  }, [cheerKey, reduce]);

  return (
    <span ref={ref} className={`mascot${open ? " is-open" : ""}${className ? " " + className : ""}`}>
      <svg viewBox="0 0 100 100" aria-hidden="true">
        <g className="mascot__body">
          <path className="mascot__lid" d="M40 33 L60 42 L56 45 L36 36 Z" fill="#ff3b18" />
          <path d="M18 40 L50 26 L82 40 L50 54 Z" fill="#141109" />
          <path d="M18 40 L18 70 L50 84 L50 54 Z" fill="#241d0e" />
          <path d="M82 40 L82 70 L50 84 L50 54 Z" fill="#141109" />
          <circle cx="34" cy="60" r="6" fill="#ffe44d" />
          <circle cx="47" cy="65" r="6" fill="#ffe44d" />
          <circle className="m-pupil" cx="34" cy="60" r="2.4" fill="#141109" />
          <circle className="m-pupil" cx="47" cy="65" r="2.4" fill="#141109" />
          <path
            d="M33 73 q7 5 13 -0.5"
            stroke="#ffe44d"
            strokeWidth="2.2"
            fill="none"
            strokeLinecap="round"
          />
        </g>
      </svg>
    </span>
  );
}
