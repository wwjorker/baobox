import type { ReactElement } from "react";

/**
 * 工具图标：贴纸风粗线条 glyph，24×24，用 currentColor 描边，
 * 分类色由外层容器给（见 theme.css 的 --img/--ocr/--pdf/--file）。
 *
 * 不打包任何图标库、不引 emoji —— 全是自己画的,和粗黑边、硬投影一脉相承。
 * 覆盖是渐进的：常见/有辨识度的工具各配专属图标,其余暂时落到「支柱通用图标」,
 * 随各支柱页迁移逐步补齐。每个工具永远有图标,名字也一直显示,不会认不出。
 */

const G: Record<string, ReactElement> = {
  // —— 支柱通用 ——
  image: (<><rect x="3" y="4" width="18" height="16" rx="2" /><circle cx="8.5" cy="9" r="1.6" /><path d="M4 18l5-5 4 3 3-3 4 4" /></>),
  screen: (<><path d="M4 8V5h3M20 8V5h-3M4 16v3h3M20 16v3h-3" /><path d="M9 15l2.5-6 2.5 6M9.8 13h3.4" /></>),
  pdf: (<><path d="M6 3h8l4 4v14H6z" /><path d="M14 3v4h4" /><path d="M9 12h6M9 15h6M9 18h4" /></>),
  file: (<><path d="M3 6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></>),

  // —— 图片 ——
  compress: (<><rect x="6" y="6" width="12" height="12" rx="1" /><path d="M3 9V4h5M21 9V4h-5M3 15v5h5M21 15v5h-5M9 12h6" /></>),
  target: (<><circle cx="12" cy="12" r="8" /><circle cx="12" cy="12" r="4" /><circle cx="12" cy="12" r="1" fill="currentColor" /></>),
  convert: (<><path d="M4 9a7 7 0 0 1 12-3l2 2M20 15a7 7 0 0 1-12 3l-2-2" /><path d="M18 4v4h-4M6 20v-4h4" /></>),
  resize: (<><rect x="3" y="3" width="18" height="18" rx="2" /><path d="M15 9V4h5M9 15v5H4" /></>),
  expand: (<><rect x="8" y="8" width="8" height="8" rx="1" /><path d="M4 8V4h4M20 8V4h-4M4 16v4h4M20 16v4h-4" /></>),
  frame: (<><rect x="3" y="3" width="18" height="18" rx="1" /><rect x="7" y="7" width="10" height="10" rx="1" /></>),
  grid9: (<><rect x="3" y="3" width="18" height="18" rx="1" /><path d="M9 3v18M15 3v18M3 9h18M3 15h18" /></>),
  stitch: (<><rect x="7" y="2" width="10" height="20" rx="1" /><path d="M7 8h10M7 14h10" /></>),
  adjust: (<><path d="M4 8h9M18 8h2M4 16h2M11 16h9" /><circle cx="15" cy="8" r="2" /><circle cx="8" cy="16" r="2" /></>),
  palette: (<><path d="M12 3a9 9 0 1 0 0 18 1.8 1.8 0 0 0 1.5-2.8 1.8 1.8 0 0 1 1.5-2.7H18a3 3 0 0 0 3-3c0-4.5-4-6.5-9-6.5z" /><circle cx="8" cy="11" r="1" fill="currentColor" /><circle cx="12" cy="8" r="1" fill="currentColor" /><circle cx="16" cy="11" r="1" fill="currentColor" /></>),
  gif: (<><rect x="3" y="5" width="18" height="14" rx="2" /><path d="M3 9h18M3 15h18M8 5v14M16 5v14" /></>),
  code: (<><path d="M8 8l-4 4 4 4M16 8l4 4-4 4M13 5l-2 14" /></>),
  scissors: (<><circle cx="6" cy="7" r="2.4" /><circle cx="6" cy="17" r="2.4" /><path d="M8 8.4L20 17M8 15.6L20 7" /></>),
  sharpen: (<><path d="M4 20L12 4l8 16z" /><path d="M8.5 20L12 12l3.5 8" /></>),
  exif: (<><path d="M12 21s7-5.6 7-11a7 7 0 1 0-14 0c0 5.4 7 11 7 11z" /><path d="M9.5 7.5l5 5M14.5 7.5l-5 5" /></>),
  ico: (<><rect x="6" y="3" width="15" height="15" rx="2" /><path d="M3 8v11a2 2 0 0 0 2 2h11" /></>),
  mosaic: (<><rect x="4" y="4" width="16" height="16" rx="1" /><path d="M4 10h16M4 15h16M9 4v16M14 4v16" /><rect x="9" y="10" width="5" height="5" fill="currentColor" stroke="none" /></>),
  watermark: (<><circle cx="12" cy="12" r="8" /><path d="M14.5 9.5a3.5 3.5 0 1 0 0 5" /></>),

  // —— OCR ——
  ocrimage: (<><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M8 16l2.5-7 2.5 7M8.8 14h3.4" /></>),
  ocrbatch: (<><rect x="7" y="3" width="14" height="14" rx="2" /><path d="M3 8v11a2 2 0 0 0 2 2h11" /><path d="M11 13l2-5 2 5M11.6 11.5h2.8" /></>),

  // —— PDF ——
  merge: (<><rect x="3" y="4" width="8" height="10" rx="1" /><rect x="13" y="10" width="8" height="10" rx="1" /><path d="M9 15l4 -1" /></>),
  organize: (<><rect x="3" y="3" width="7" height="8" rx="1" /><rect x="14" y="3" width="7" height="8" rx="1" /><rect x="3" y="14" width="7" height="7" rx="1" /><path d="M14 17h7M17.5 14v7" /></>),
  rotate: (<><path d="M20 12a8 8 0 1 1-2.4-5.7" /><path d="M20 4v4h-4" /><rect x="9" y="9" width="6" height="6" rx="1" /></>),
  reverse: (<><path d="M6 8h12l-3-3M18 16H6l3 3" /></>),
  nup: (<><rect x="3" y="3" width="18" height="18" rx="1" /><path d="M12 3v18M3 12h18" /></>),
  lock: (<><rect x="5" y="11" width="14" height="9" rx="2" /><path d="M8 11V7a4 4 0 0 1 8 0v4" /></>),
  unlock: (<><rect x="5" y="11" width="14" height="9" rx="2" /><path d="M8 11V7a4 4 0 0 1 7.5-1.8" /></>),
  crop: (<><path d="M6 2v14a2 2 0 0 0 2 2h14" /><path d="M2 6h14a2 2 0 0 1 2 2v14" /></>),
  blank: (<><path d="M6 3h12v18H6z" /><path d="M12 9v6M9 12h6" /></>),
  sparkle: (<><path d="M6 3h8l4 4v14H6z" /><path d="M14 3v4h4" /><path d="M11.5 11l1 2.2 2.2 1-2.2 1-1 2.2-1-2.2-2.2-1 2.2-1z" /></>),
  wrench: (<><path d="M15 6.5a3.2 3.2 0 0 0-4.2 4.2L4 17.5V20h2.5l6.8-6.8A3.2 3.2 0 0 0 17.5 9l-2 2-2-2 2-2z" /></>),
  stampnum: (<><path d="M6 3h8l4 4v14H6z" /><path d="M14 3v4h4" /><rect x="9" y="13" width="6" height="4" rx="1" /></>),
  textlines: (<><rect x="4" y="3" width="16" height="18" rx="2" /><path d="M8 8h8M8 12h8M8 16h5" /></>),

  // —— 文件 ——
  rename: (<><path d="M3 8l7-4 10 3.5v6L10 20l-7-3.6z" /><path d="M8 10.5l6 2" /><circle cx="9" cy="9" r="0.6" fill="currentColor" /></>),
  dedupe: (<><rect x="4" y="4" width="10" height="10" rx="1" /><rect x="9" y="9" width="8" height="8" rx="1" /><circle cx="17" cy="17" r="3" /><path d="M19.2 19.2L22 22" /></>),
  hash: (<><rect x="3" y="3" width="18" height="18" rx="2" /><path d="M9 8v8M14 8v8M7 11h10M7 15h10" /></>),
  zipbox: (<><rect x="5" y="3" width="14" height="18" rx="2" /><path d="M12 3v4M10 7h4M10 10h4M10 13h4" /><rect x="10.5" y="15.5" width="3" height="4" rx="0.6" /></>),
  clock: (<><circle cx="12" cy="12" r="8" /><path d="M12 8v4l3 2" /></>),
  folderplus: (<><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /><path d="M12 11v5M9.5 13.5h5" /></>),
  tree: (<><rect x="3" y="3" width="6" height="4" rx="1" /><rect x="15" y="9" width="6" height="4" rx="1" /><rect x="15" y="16" width="6" height="4" rx="1" /><path d="M6 7v11h9M6 11h9" /></>),
  findreplace: (<><path d="M4 6h6M4 10h4M4 14h6" /><path d="M14 9l3 3-3 3M17 12h-5" /></>),
  table: (<><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M3 9h18M3 14h18M9 4v16M15 4v16" /></>),
  qr: (<><path d="M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4z" /><path d="M14 14h2v2h-2M18 14h2v2h-2M14 18h2v2h-2M18 18h2v2h-2" /></>),
  textfix: (<><path d="M4 7h9M4 12h6M4 17h9" /><path d="M15 8l4 4M19 8l-4 4" /></>),
  zhconv: (<><path d="M4 6h6M7 6v11M4 12h6" /><path d="M14 8h6M17 8v9M14 17h6M15.5 12h3" /></>),
  sortlines: (<><path d="M5 7h13M5 12h9M5 17h5" /><path d="M18 14l2.5 3 2.5-3" /></>),
  shred: (<><rect x="4" y="3" width="16" height="6" rx="1" /><path d="M2 12h20" /><path d="M6 15v5M10 15v3M14 15v5M18 15v3" /></>),
  splitfile: (<><rect x="8" y="3" width="8" height="6" rx="1" /><rect x="3" y="15" width="6" height="6" rx="1" /><rect x="15" y="15" width="6" height="6" rx="1" /><path d="M12 9v2M12 11l-6 3M12 11l6 3" /></>),

  // —— 成对「产出 / 反向」工具的区分图标 ——
  // 同一根支柱下,make 与 split、to 与 from 若共用一个 glyph 就完全认不出谁是谁,
  // 所以「反向那一侧」各给一个带方向暗示的专属图标。
  // 图片装进 PDF：pdf 页里嵌着一张照片
  pdfimg: (<><path d="M6 3h8l4 4v14H6z" /><path d="M14 3v4h4" /><circle cx="10" cy="12" r="1.1" /><path d="M8 18l2.3-2.8 1.8 1.4 2.4-2.9" /></>),
  // 从 PDF 里抠出内嵌图片：照片 + 斜向外的箭头
  extractimg: (<><rect x="3" y="10" width="10" height="9" rx="1" /><circle cx="6.2" cy="13" r="1" /><path d="M3 17l3-2.4 2.4 1.9" /><path d="M14 9h6v6M20 9l-6 6" /></>),
  // GIF 拆帧：主胶片旁分出一格
  gifsplit: (<><rect x="3" y="6" width="10" height="12" rx="1" /><path d="M3 10h10M3 14h10M7 6v12" /><rect x="16" y="9" width="5" height="6" rx="1" /><path d="M13 12h3" /></>),
  // 识别二维码：三个定位角 + 一道扫描线
  qrscan: (<><path d="M4 4h5v5H4zM15 4h5v5h-5zM4 15h5v5H4z" /><path d="M2 12h20" /></>),
  // 解压：开盖的箱子,东西往外抬
  unzipbox: (<><rect x="5" y="8" width="14" height="12" rx="1.5" /><path d="M5 8l3-4h8l3 4" /><path d="M12 12v5M9.5 14.5L12 12l2.5 2.5" /></>),
  // 删除/保留指定页：pdf 页 + 勾选,表示挑出要的那几页（区别于「页面整理」的缩略图网格）
  pagepick: (<><path d="M6 3h8l4 4v14H6z" /><path d="M14 3v4h4" /><path d="M9 11h6" /><path d="M9 15.5l1.6 1.6 3.4-3.6" /></>),
  // 自动色阶：直方图柱 + 基线（区别于「调色」的手动滑块）
  autolevel: (<><path d="M4 20V11M9 20V5M14 20V14M19 20V8" /><path d="M3 20h18" /></>),
  // 改宽高比：外框套内框、比例不同（区别于「缩放」的箭头撑大、「圆角描边」的等距套框）
  aspect: (<><rect x="3" y="6" width="18" height="12" rx="1" /><rect x="8" y="6" width="8" height="12" /></>),
};

const MAP: Record<string, string> = {
  "image.adjust": "adjust",
  "image.aspect": "aspect",
  "image.autolevel": "autolevel",
  "image.base64": "code",
  "image.compress": "compress",
  "image.compress-target": "target",
  "image.convert": "convert",
  "image.expand": "expand",
  "image.frame": "frame",
  "image.gif-make": "gif",
  "image.gif-split": "gifsplit",
  "image.grid": "grid9",
  "image.ico": "ico",
  "image.palette": "palette",
  "image.redact": "mosaic",
  "image.resize": "resize",
  "image.sharpen": "sharpen",
  "image.stitch": "stitch",
  "image.strip-exif": "exif",
  "image.trim": "scissors",
  "image.watermark": "watermark",
  "ocr.batch": "ocrbatch",
  "ocr.image": "ocrimage",
  "ocr.screen": "screen",
  "pdf.blank": "blank",
  "pdf.clean-meta": "sparkle",
  "pdf.compress": "compress",
  "pdf.crop": "crop",
  "pdf.decrypt": "unlock",
  "pdf.encrypt": "lock",
  "pdf.extract-images": "extractimg",
  "pdf.from-image": "pdfimg",
  "pdf.merge": "merge",
  "pdf.nup": "nup",
  "pdf.ocr-layer": "ocrimage",
  "pdf.pages": "pagepick",
  "pdf.repair": "wrench",
  "pdf.reverse": "reverse",
  "pdf.rotate": "rotate",
  "pdf.split": "organize",
  "pdf.stamp": "stampnum",
  "pdf.to-image": "image",
  "pdf.to-text": "textlines",
  "file.data": "table",
  "file.dedupe": "dedupe",
  "file.encoding": "textfix",
  "file.hash": "hash",
  "file.join": "merge",
  "file.lines": "sortlines",
  "file.mkdirs": "folderplus",
  "file.qr-make": "qr",
  "file.qr-read": "qrscan",
  "file.rename": "rename",
  "file.replace": "findreplace",
  "file.shred": "shred",
  "file.split": "splitfile",
  "file.touch": "clock",
  "file.tree": "tree",
  "file.unzip": "unzipbox",
  "file.zhconv": "zhconv",
  "file.zip-make": "zipbox",
};

export type Pillar = "image" | "ocr" | "pdf" | "file";

/** 工具 id 的所属支柱（即分类色）。 */
export function pillarOf(id: string): Pillar {
  const p = id.split(".")[0];
  return p === "image" || p === "ocr" || p === "pdf" || p === "file" ? p : "file";
}

export function ToolIcon({ id, className }: { id: string; className?: string }) {
  const key = MAP[id] ?? pillarOf(id);
  return (
    <svg
      viewBox="0 0 24 24"
      className={className}
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {G[key] ?? G[pillarOf(id)]}
    </svg>
  );
}
