/**
 * 完成时的纸屑。一块贴在 body 上的全屏 canvas，按需喷、喷完自己停。
 * 尊重「减少动态效果」：prefers-reduced-motion 下不喷。
 */
interface Bit {
  x: number;
  y: number;
  vx: number;
  vy: number;
  s: number;
  rot: number;
  vr: number;
  c: string;
  life: number;
}

const COLORS = ["#ffe44d", "#ff3b18", "#141109", "#3d8bf0", "#17b877"];
let canvas: HTMLCanvasElement | null = null;
let ctx: CanvasRenderingContext2D | null = null;
let bits: Bit[] = [];
let raf = 0;

function ensure() {
  if (canvas) return;
  canvas = document.createElement("canvas");
  canvas.style.cssText =
    "position:fixed;inset:0;width:100%;height:100%;pointer-events:none;z-index:90";
  document.body.appendChild(canvas);
  ctx = canvas.getContext("2d");
  const resize = () => {
    if (!canvas) return;
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
  };
  resize();
  window.addEventListener("resize", resize);
}

export function burstConfetti(x?: number, y?: number, n = 60) {
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  ensure();
  const cx = x ?? window.innerWidth / 2;
  const cy = y ?? window.innerHeight / 2;
  for (let i = 0; i < n; i++) {
    bits.push({
      x: cx,
      y: cy,
      vx: (Math.random() - 0.5) * 13,
      vy: -Math.random() * 14 - 4,
      s: 5 + Math.random() * 7,
      rot: Math.random() * 6,
      vr: (Math.random() - 0.5) * 0.5,
      c: COLORS[(Math.random() * COLORS.length) | 0],
      life: 100,
    });
  }
  if (!raf) tick();
}

function tick() {
  if (!ctx || !canvas) return;
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  bits = bits.filter((p) => p.life > 0);
  for (const p of bits) {
    p.vy += 0.42;
    p.x += p.vx;
    p.y += p.vy;
    p.rot += p.vr;
    p.life--;
    ctx.save();
    ctx.translate(p.x, p.y);
    ctx.rotate(p.rot);
    ctx.fillStyle = p.c;
    ctx.fillRect(-p.s / 2, -p.s / 2, p.s, p.s * 0.6);
    ctx.restore();
  }
  if (bits.length) {
    raf = requestAnimationFrame(tick);
  } else {
    raf = 0;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
  }
}
