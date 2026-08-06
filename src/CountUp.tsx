import { useEffect, useRef, useState } from "react";

/**
 * 数字滚动：值增长时从旧值缓动到新值,而不是「啪」地跳过去。
 *
 * 用在「已省下 X」——压完一批图,那个数字滚一下涨上去,是最有成就感、
 * 也最好当截图传播的一刻。首次渲染不滚(直接显终值),只有后续变化才滚。
 * 尊重「减少动态效果」:reduced-motion 下直接跳到终值。
 */
export function CountUp({
  value,
  format,
  ms = 650,
}: {
  value: number;
  /** 把中间的数值格式化成要显示的字符串(比如 fmtSize) */
  format: (n: number) => string;
  ms?: number;
}) {
  const [disp, setDisp] = useState(value);
  // 记住上一帧真正显示到的值,作为下次滚动的起点——连续触发也不会跳
  const from = useRef(value);

  useEffect(() => {
    const target = value;
    const start = from.current;
    if (start === target) return;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      setDisp(target);
      from.current = target;
      return;
    }
    let raf = 0;
    const t0 = performance.now();
    const tick = (now: number) => {
      const p = Math.min(1, (now - t0) / ms);
      const eased = 1 - Math.pow(1 - p, 3); // easeOutCubic
      const v = start + (target - start) * eased;
      setDisp(v);
      from.current = v;
      if (p < 1) raf = requestAnimationFrame(tick);
      else from.current = target;
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [value, ms]);

  return <>{format(disp)}</>;
}
