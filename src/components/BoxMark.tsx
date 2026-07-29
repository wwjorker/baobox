/**
 * 品牌母题：一只歪着的百宝箱。
 *
 * 原先左上角是个白方块里放一个字母 B —— 那是最没有记忆点的一种标志，
 * 换成任何别的软件都成立，正是「一眼看出是 AI 随手生成的」那种做法。
 * 视觉打磨稿第 10 条本来就写了要用歪斜的箱子当母题，一直没做。
 *
 * 手写路径而不是引图标库：全套只有六条线，为它拖进来一个依赖不划算，
 * 而且粗描边的宽度要跟整体的三级边框对得上，现成图标都太细。
 */
export function BoxMark({
  size = 22,
  className,
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2.4}
      strokeLinecap="square"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {/* 提手 */}
      <path d="M9 6V4h6v2" />
      {/* 箱体 */}
      <rect x="2.6" y="6" width="18.8" height="14" />
      {/* 盖缝 */}
      <path d="M2.6 11h18.8" />
      {/* 锁扣。小尺寸下四个元素挤在一起会糊成一团黑，
          标题栏那个只有 15px，去掉这一笔反而更像箱子。 */}
      {size >= 28 && <path d="M10 9.6h4v3.2h-4z" />}
    </svg>
  );
}
