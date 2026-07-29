/**
 * 工具注册表
 *
 * 每个工具用一份声明式描述定义清楚：属于哪根支柱、收什么文件、
 * 有哪些参数、调后端哪个命令。界面由描述自动渲染，
 * 所以新增一个工具的成本 ≈ 往这个数组里加一项 + 写对应的 Rust 命令。
 *
 * 这层抽象的质量直接决定后面 80 个功能的开发速度，是阶段 0 的重点。
 */

export type Pillar = "image" | "ocr" | "pdf" | "file";

/** 工具的可用状态。v1.0 还在开发中，界面必须诚实标注哪些能用。 */
export type ToolStatus = "ready" | "wip" | "planned";

export type OptionDef =
  | {
      kind: "number";
      id: string;
      label: string;
      min: number;
      max: number;
      step: number;
      def: number;
      unit?: string;
    }
  | {
      kind: "choice";
      id: string;
      label: string;
      choices: { value: string; label: string }[];
      def: string;
    }
  | { kind: "toggle"; id: string; label: string; def: boolean }
  | { kind: "text"; id: string; label: string; def: string; placeholder?: string }
  /** 选项来自运行时探测，比如系统装了哪些 OCR 语言包 */
  | { kind: "dynamic-choice"; id: string; label: string; source: "ocrLanguages"; def: string };

export interface ToolDef {
  /** 同时作为 i18n 查找键：tool.<id>.name / tool.<id>.desc */
  id: string;
  pillar: Pillar;
  /** 接受的扩展名，空数组表示接受任意文件 */
  accepts: string[];
  /** 对应的 Tauri 后端命令 */
  command: string;
  options: OptionDef[];
  status: ToolStatus;
  /** 差异化卖点，界面上会打标 */
  highlight?: boolean;
  /**
   * 产物形态。多数工具是文件进文件出，但 OCR 这类的产物是文本，
   * 用户要的是能直接看见和复制的内容，而不是一个待打开的文件。
   */
  output?: "file" | "text";
  /**
   * 未实现的具体原因（i18n 键）。
   * 有些工具不是「还没排到」，而是有明确的技术或安全判断，
   * 界面上把话说清楚，比给一句通用的「敬请期待」诚实得多。
   */
  notReadyReason?: string;
  /**
   * N→1：多个输入合出一份产物。
   *
   * 这类工具不能按「每个文件各自变大变小」来汇报——合并三份 PDF 得到一份更大的
   * 文件是理所当然的，把它显示成「体积没有变小」是在报一个不存在的问题。
   */
  aggregate?: boolean;
  /**
   * 输入顺序影响结果。合并和图片转 PDF 的页序就是文件顺序，
   * 说明文案里承诺了「排好序」，界面就必须给得出排序的手段。
   */
  ordered?: boolean;
  /**
   * 产物是原件的等价替换，因此体积差是真的省下来的。
   *
   * 只有压缩、转格式、缩放这类成立。OCR 产出的是文本、原图还在，
   * 加水印产出的是另一张图——把这些也算进「已省下」，
   * 首页那个数字就成了假的。它是最好的传播素材，更不能虚报。
   */
  savesSpace?: boolean;
}

const IMG = ["jpg", "jpeg", "png", "webp", "bmp", "tif", "tiff"];
const PDF = ["pdf"];

export const TOOLS: ToolDef[] = [
  // ---------------------------------------------------------- 图片（7）
  {
    id: "image.compress-target",
    pillar: "image",
    accepts: IMG,
    command: "img_compress_target",
    status: "ready",
    highlight: true,
    savesSpace: true,
    options: [
      { kind: "number", id: "targetKb", label: "opt.targetSize", min: 50, max: 5000, step: 50, def: 500, unit: "KB" },
      {
        kind: "choice",
        id: "format",
        label: "opt.format",
        def: "webp",
        choices: [
          { value: "keep", label: "opt.formatKeep" },
          { value: "webp", label: "WebP" },
          { value: "jpeg", label: "JPEG" },
        ],
      },
    ],
  },
  {
    id: "image.compress",
    pillar: "image",
    accepts: IMG,
    command: "img_compress",
    status: "ready",
    savesSpace: true,
    options: [
      { kind: "number", id: "quality", label: "opt.quality", min: 1, max: 100, step: 1, def: 82 },
    ],
  },
  {
    id: "image.convert",
    pillar: "image",
    accepts: IMG,
    command: "img_convert",
    status: "ready",
    savesSpace: true,
    options: [
      {
        kind: "choice",
        id: "format",
        label: "opt.format",
        def: "webp",
        choices: [
          { value: "jpeg", label: "JPEG" },
          { value: "png", label: "PNG" },
          { value: "webp", label: "WebP" },
        ],
      },
    ],
  },
  {
    id: "image.resize",
    pillar: "image",
    accepts: IMG,
    command: "img_resize",
    status: "ready",
    savesSpace: true,
    options: [
      { kind: "number", id: "longEdge", label: "opt.longEdge", min: 100, max: 8000, step: 100, def: 1920, unit: "px" },
    ],
  },
  {
    id: "image.strip-exif",
    pillar: "image",
    accepts: IMG,
    command: "img_strip_exif",
    status: "ready",
    highlight: true,
    savesSpace: true,
    options: [{ kind: "toggle", id: "keepOrientation", label: "opt.keepOrientation", def: true }],
  },
  {
    id: "image.watermark",
    pillar: "image",
    accepts: IMG,
    command: "img_watermark",
    status: "ready",
    options: [
      { kind: "text", id: "text", label: "opt.watermarkText", def: "", placeholder: "opt.watermarkPlaceholder" },
      { kind: "number", id: "opacity", label: "opt.opacity", min: 5, max: 100, step: 5, def: 30, unit: "%" },
      { kind: "toggle", id: "tile", label: "opt.tile", def: true },
    ],
  },
  {
    id: "image.redact",
    pillar: "image",
    accepts: IMG,
    command: "img_redact",
    status: "ready",
    highlight: true,
    options: [
      {
        kind: "choice",
        id: "mode",
        label: "opt.redactMode",
        def: "pixelate",
        choices: [
          { value: "pixelate", label: "opt.redactPixelate" },
          { value: "blackout", label: "opt.redactBlackout" },
        ],
      },
    ],
  },

  // ------------------------------------------------------------ OCR（3）
  {
    id: "ocr.image",
    pillar: "ocr",
    accepts: IMG,
    command: "ocr_image",
    status: "ready",
    highlight: true,
    output: "text",
    options: [{ kind: "dynamic-choice", id: "lang", label: "opt.ocrLang", source: "ocrLanguages", def: "" }],
  },
  {
    id: "ocr.batch",
    pillar: "ocr",
    accepts: IMG,
    command: "ocr_batch",
    status: "ready",
    output: "text",
    options: [{ kind: "dynamic-choice", id: "lang", label: "opt.ocrLang", source: "ocrLanguages", def: "" }],
  },
  { id: "ocr.screen", pillar: "ocr", accepts: [], command: "ocr_region", status: "ready", highlight: true, options: [] },

  // ------------------------------------------------------------ PDF（8）
  { id: "pdf.merge", pillar: "pdf", accepts: PDF, command: "pdf_merge", status: "ready", aggregate: true, ordered: true, options: [] },
  { id: "pdf.split", pillar: "pdf", accepts: PDF, command: "pdf_split", status: "ready", options: [] },
  {
    id: "pdf.rotate",
    pillar: "pdf",
    accepts: PDF,
    command: "pdf_rotate",
    status: "ready",
    options: [
      {
        kind: "choice",
        id: "degrees",
        label: "opt.rotate",
        def: "90",
        choices: [
          { value: "90", label: "90°" },
          { value: "180", label: "180°" },
          { value: "270", label: "270°" },
        ],
      },
    ],
  },
  {
    id: "pdf.compress",
    pillar: "pdf",
    accepts: PDF,
    command: "pdf_compress",
    status: "ready",
    savesSpace: true,
    options: [{ kind: "number", id: "quality", label: "opt.quality", min: 1, max: 100, step: 1, def: 75 }],
  },
  {
    id: "pdf.to-image",
    pillar: "pdf",
    accepts: PDF,
    command: "pdf_to_image",
    status: "ready",
    options: [{ kind: "number", id: "dpi", label: "opt.dpi", min: 72, max: 600, step: 24, def: 150, unit: "DPI" }],
  },
  { id: "pdf.from-image", pillar: "pdf", accepts: IMG, command: "pdf_from_image", status: "ready", aggregate: true, ordered: true, options: [] },
  {
    id: "pdf.to-text",
    pillar: "pdf",
    accepts: PDF,
    command: "pdf_to_text",
    status: "ready",
    output: "text",
    options: [],
  },
  {
    id: "pdf.decrypt",
    pillar: "pdf",
    accepts: PDF,
    command: "pdf_decrypt",
    status: "ready",
    options: [{ kind: "text", id: "password", label: "opt.password", def: "" }],
  },
  {
    id: "pdf.encrypt",
    pillar: "pdf",
    accepts: PDF,
    command: "pdf_encrypt",
    status: "planned",
    notReadyReason: "status.encryptWhy",
    options: [{ kind: "text", id: "password", label: "opt.password", def: "" }],
  },
  {
    id: "pdf.stamp",
    pillar: "pdf",
    accepts: PDF,
    command: "pdf_stamp",
    status: "ready",
    highlight: true,
    options: [
      { kind: "text", id: "text", label: "opt.watermarkText", def: "", placeholder: "opt.watermarkPlaceholder" },
      { kind: "toggle", id: "pageNumbers", label: "opt.pageNumbers", def: true },
      { kind: "number", id: "opacity", label: "opt.opacity", min: 5, max: 100, step: 5, def: 25, unit: "%" },
    ],
  },

  // ----------------------------------------------------------- 文件（2）
  { id: "file.rename", pillar: "file", accepts: [], command: "rename_apply", status: "ready", options: [] },
  { id: "file.dedupe", pillar: "file", accepts: [], command: "find_duplicates", status: "ready", options: [] },
];

export const PILLARS: Pillar[] = ["image", "ocr", "pdf", "file"];

export function toolsOf(pillar: Pillar): ToolDef[] {
  return TOOLS.filter((t) => t.pillar === pillar);
}

export function findTool(id: string): ToolDef | undefined {
  return TOOLS.find((t) => t.id === id);
}

/** 支柱图标用单个汉字，避免引入图标库，也更贴合粗野贴纸的气质 */
export const PILLAR_GLYPH: Record<Pillar, string> = {
  image: "图",
  ocr: "识",
  pdf: "P",
  file: "件",
};
