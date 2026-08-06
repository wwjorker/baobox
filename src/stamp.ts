/**
 * 完成印章。操作成功时,屏幕中央「啪」地盖一个章,停一下再淡出。
 *
 * 和纸屑一样是命令式的:不进 React 树,在完成的那一处直接调 stampDone()。
 * 走 Web Animations API,不打包任何素材。风格跟着粗野贴纸走——米黄底、
 * 粗黑边、硬投影、朱红对勾,略微歪着盖,像真章。
 * 尊重「减少动态效果」:reduced-motion 下不盖。
 */
const STAMP_SVG = `<svg width="118" height="118" viewBox="0 0 120 120" fill="none" xmlns="http://www.w3.org/2000/svg">
  <rect x="10" y="10" width="100" height="100" rx="20" fill="#fff9e8" stroke="#141109" stroke-width="6"/>
  <path d="M34 63 L52 83 L88 37" stroke="#ff3b18" stroke-width="12" stroke-linecap="round" stroke-linejoin="round"/>
</svg>`;

export function stampDone() {
  if (typeof window === "undefined") return;
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const el = document.createElement("div");
  el.innerHTML = STAMP_SVG;
  el.style.cssText =
    "position:fixed;left:50%;top:45%;z-index:95;pointer-events:none;" +
    "filter:drop-shadow(4px 4px 0 rgba(20,17,9,.4))";
  document.body.appendChild(el);
  const anim = el.animate(
    [
      { opacity: 0, transform: "translate(-50%,-50%) scale(1.7) rotate(-9deg)" },
      { opacity: 1, transform: "translate(-50%,-50%) scale(0.9) rotate(-9deg)", offset: 0.16 },
      { opacity: 1, transform: "translate(-50%,-50%) scale(1) rotate(-9deg)", offset: 0.26 },
      { opacity: 1, transform: "translate(-50%,-50%) scale(1) rotate(-9deg)", offset: 0.72 },
      { opacity: 0, transform: "translate(-50%,-50%) scale(1.12) rotate(-9deg)", offset: 1 },
    ],
    { duration: 1050, easing: "cubic-bezier(.2,.8,.2,1)" },
  );
  const done = () => el.remove();
  anim.onfinish = done;
  anim.oncancel = done;
}
