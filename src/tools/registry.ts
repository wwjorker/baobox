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
  /**
   * 收的是文件夹本身，不是里面的文件。
   *
   * 目录树导出就是这一类。其余工具拖进文件夹要展开成文件，
   * 而这个工具展开了就什么都不剩了——它要的正是那个文件夹。
   */
  takesFolders?: boolean;
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

  // 中文社区天天在用、欧美图片工具普遍不做的两个。
  // 差异化不是靠功能多，是靠别人不做而这里的人真的需要。
  {
    id: "image.grid",
    pillar: "image",
    accepts: IMG,
    command: "img_grid",
    status: "ready",
    highlight: true,
    options: [
      { kind: "number", id: "rows", label: "opt.gridRows", min: 1, max: 6, step: 1, def: 3 },
      { kind: "number", id: "cols", label: "opt.gridCols", min: 1, max: 6, step: 1, def: 3 },
      { kind: "toggle", id: "squareFirst", label: "opt.squareFirst", def: true },
    ],
  },
  {
    id: "image.stitch",
    pillar: "image",
    accepts: IMG,
    command: "img_stitch",
    status: "ready",
    highlight: true,
    aggregate: true,
    ordered: true,
    options: [
      {
        kind: "choice",
        id: "direction",
        label: "opt.stitchDir",
        def: "vertical",
        choices: [
          { value: "vertical", label: "opt.stitchVertical" },
          { value: "horizontal", label: "opt.stitchHorizontal" },
        ],
      },
      { kind: "number", id: "gap", label: "opt.stitchGap", min: 0, max: 60, step: 2, def: 0, unit: "px" },
    ],
  },
  {
    id: "image.trim",
    pillar: "image",
    accepts: IMG,
    command: "img_trim",
    status: "ready",
    savesSpace: true,
    options: [
      { kind: "number", id: "tolerance", label: "opt.trimTolerance", min: 0, max: 60, step: 2, def: 10 },
    ],
  },
  {
    id: "image.frame",
    pillar: "image",
    accepts: IMG,
    command: "img_frame",
    status: "ready",
    options: [
      { kind: "number", id: "radius", label: "opt.cornerRadius", min: 0, max: 50, step: 1, def: 8, unit: "%" },
      { kind: "number", id: "border", label: "opt.borderWidth", min: 0, max: 80, step: 2, def: 0, unit: "px" },
      { kind: "toggle", id: "borderDark", label: "opt.borderDark", def: false },
    ],
  },
  {
    id: "image.aspect",
    pillar: "image",
    accepts: IMG,
    command: "img_aspect",
    status: "ready",
    options: [
      {
        kind: "choice",
        id: "ratio",
        label: "opt.aspectRatio",
        def: "1:1",
        choices: [
          { value: "1:1", label: "1:1" },
          { value: "4:3", label: "4:3" },
          { value: "3:4", label: "3:4" },
          { value: "16:9", label: "16:9" },
          { value: "9:16", label: "9:16" },
        ],
      },
    ],
  },
  {
    id: "image.palette",
    pillar: "image",
    accepts: IMG,
    command: "img_palette",
    status: "ready",
    output: "text",
    options: [
      { kind: "number", id: "count", label: "opt.paletteCount", min: 1, max: 12, step: 1, def: 5 },
    ],
  },
  { id: "image.base64", pillar: "image", accepts: IMG, command: "img_base64", status: "ready", output: "text", options: [] },
  {
    id: "image.expand",
    pillar: "image",
    accepts: IMG,
    command: "img_expand",
    status: "ready",
    options: [
      {
        kind: "choice",
        id: "ratio",
        label: "opt.aspectRatio",
        def: "1:1",
        choices: [
          { value: "1:1", label: "1:1" },
          { value: "4:3", label: "4:3" },
          { value: "3:4", label: "3:4" },
          { value: "16:9", label: "16:9" },
          { value: "9:16", label: "9:16" },
        ],
      },
      { kind: "toggle", id: "fillDark", label: "opt.fillDark", def: false },
    ],
  },
  {
    id: "image.gif-split",
    pillar: "image",
    accepts: ["gif"],
    command: "gif_split",
    status: "ready",
    options: [
      { kind: "number", id: "every", label: "opt.everyNth", min: 1, max: 20, step: 1, def: 1 },
    ],
  },
  {
    id: "image.gif-make",
    pillar: "image",
    accepts: IMG,
    command: "gif_make",
    status: "ready",
    aggregate: true,
    ordered: true,
    options: [
      { kind: "number", id: "delay", label: "opt.frameDelay", min: 20, max: 2000, step: 20, def: 200, unit: "ms" },
    ],
  },
  {
    id: "image.ico",
    pillar: "image",
    accepts: IMG,
    command: "img_ico",
    status: "ready",
    options: [],
  },
  {
    id: "image.adjust",
    pillar: "image",
    accepts: IMG,
    command: "img_adjust",
    status: "ready",
    options: [
      { kind: "number", id: "brightness", label: "opt.brightness", min: -100, max: 100, step: 5, def: 0 },
      { kind: "number", id: "contrast", label: "opt.contrast", min: -100, max: 100, step: 5, def: 0 },
      { kind: "number", id: "saturation", label: "opt.saturation", min: -100, max: 100, step: 5, def: 0 },
      {
        kind: "choice",
        id: "filter",
        label: "opt.filter",
        def: "none",
        choices: [
          { value: "none", label: "opt.filterNone" },
          { value: "gray", label: "opt.grayscale" },
          { value: "sepia", label: "opt.sepia" },
          { value: "invert", label: "opt.invert" },
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
  // 放在 PDF 而不是 OCR 支柱下：手里有扫描件的人是奔着 PDF 来的，
  // 首页拖一份 PDF 进来也是翻到这里。
  {
    id: "pdf.ocr-layer",
    pillar: "pdf",
    accepts: PDF,
    command: "pdf_ocr_layer",
    status: "ready",
    highlight: true,
    options: [
      { kind: "dynamic-choice", id: "lang", label: "opt.ocrLang", source: "ocrLanguages", def: "" },
    ],
  },
  {
    id: "pdf.extract-images",
    pillar: "pdf",
    accepts: PDF,
    command: "pdf_extract_images",
    status: "ready",
    options: [
      { kind: "number", id: "minPx", label: "opt.minPx", min: 0, max: 2000, step: 50, def: 100, unit: "px" },
    ],
  },
  { id: "pdf.reverse", pillar: "pdf", accepts: PDF, command: "pdf_reverse", status: "ready", options: [] },
  {
    id: "pdf.repair",
    pillar: "pdf",
    accepts: PDF,
    command: "pdf_repair",
    status: "ready",
    options: [],
  },
  {
    id: "pdf.nup",
    pillar: "pdf",
    accepts: PDF,
    command: "pdf_nup",
    status: "ready",
    options: [
      {
        kind: "choice",
        id: "layout",
        label: "opt.nupLayout",
        def: "2x1",
        choices: [
          { value: "2x1", label: "2 合 1" },
          { value: "1x2", label: "2 合 1（上下）" },
          { value: "2x2", label: "4 合 1" },
          { value: "3x3", label: "9 合 1" },
          { value: "2x4", label: "8 合 1" },
        ],
      },
      { kind: "number", id: "gap", label: "opt.nupGap", min: 0, max: 60, step: 2, def: 10, unit: "pt" },
    ],
  },
  {
    id: "pdf.blank",
    pillar: "pdf",
    accepts: PDF,
    command: "pdf_blank",
    status: "ready",
    options: [
      { kind: "text", id: "after", label: "opt.blankAfter", def: "", placeholder: "opt.blankAfterHint" },
      { kind: "number", id: "count", label: "opt.blankCount", min: 1, max: 10, step: 1, def: 1 },
    ],
  },
  {
    id: "pdf.clean-meta",
    pillar: "pdf",
    accepts: PDF,
    command: "pdf_clean_meta",
    status: "ready",
    highlight: true,
    options: [{ kind: "toggle", id: "keepDates", label: "opt.keepDates", def: false }],
  },
  {
    id: "pdf.crop",
    pillar: "pdf",
    accepts: PDF,
    command: "pdf_crop",
    status: "ready",
    options: [
      { kind: "number", id: "margin", label: "opt.keepMargin", min: 0, max: 72, step: 2, def: 8, unit: "pt" },
    ],
  },
  {
    id: "pdf.pages",
    pillar: "pdf",
    accepts: PDF,
    command: "pdf_pages",
    status: "ready",
    options: [
      { kind: "text", id: "pages", label: "opt.pageSpec", def: "", placeholder: "opt.pageSpecHint" },
      { kind: "toggle", id: "keepMode", label: "opt.keepMode", def: false },
    ],
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
  {
    id: "file.encoding",
    pillar: "file",
    accepts: ["txt", "csv", "log", "md", "json", "xml", "srt", "ass", "ini", "html", "css", "js"],
    command: "text_fix_encoding",
    status: "ready",
    highlight: true,
    options: [{ kind: "toggle", id: "addBom", label: "opt.addBom", def: true }],
  },
  {
    id: "file.zhconv",
    pillar: "file",
    accepts: ["txt", "csv", "log", "md", "srt", "ass", "json", "xml", "html"],
    command: "text_zhconv",
    status: "ready",
    highlight: true,
    options: [
      {
        kind: "choice",
        id: "target",
        label: "opt.zhTarget",
        def: "hant",
        choices: [
          { value: "hant", label: "opt.zhToHant" },
          { value: "hans", label: "opt.zhToHans" },
        ],
      },
    ],
  },
  {
    id: "file.replace",
    pillar: "file",
    accepts: ["txt", "csv", "log", "md", "json", "xml", "srt", "ass", "ini", "html", "css", "js"],
    command: "text_replace",
    status: "ready",
    options: [
      { kind: "text", id: "find", label: "opt.find", def: "", placeholder: "opt.findHint" },
      { kind: "text", id: "replace", label: "opt.replaceWith", def: "" },
      { kind: "toggle", id: "useRegex", label: "opt.useRegex", def: false },
      { kind: "toggle", id: "caseSensitive", label: "opt.caseSensitive", def: true },
    ],
  },
  {
    id: "file.tree",
    pillar: "file",
    accepts: [],
    command: "dir_tree",
    status: "ready",
    output: "text",
    takesFolders: true,
    options: [
      { kind: "number", id: "depth", label: "opt.treeDepth", min: 1, max: 12, step: 1, def: 4 },
      { kind: "toggle", id: "showSize", label: "opt.showSize", def: true },
    ],
  },
  {
    id: "file.qr-make",
    pillar: "file",
    accepts: ["txt", "csv"],
    command: "qr_generate",
    status: "ready",
    highlight: true,
    options: [
      { kind: "number", id: "size", label: "opt.qrSize", min: 128, max: 1200, step: 64, def: 512, unit: "px" },
    ],
  },
  {
    id: "file.qr-read",
    pillar: "file",
    accepts: IMG,
    command: "qr_decode",
    status: "ready",
    output: "text",
    options: [],
  },
  {
    id: "file.lines",
    pillar: "file",
    accepts: ["txt", "csv", "log", "md"],
    command: "text_lines",
    status: "ready",
    options: [
      { kind: "toggle", id: "dedupe", label: "opt.lineDedupe", def: true },
      { kind: "toggle", id: "sort", label: "opt.lineSort", def: false },
      { kind: "toggle", id: "count", label: "opt.lineCount", def: false },
    ],
  },
  {
    id: "file.split",
    pillar: "file",
    accepts: [],
    command: "file_split",
    status: "ready",
    options: [
      { kind: "number", id: "partMb", label: "opt.partSize", min: 1, max: 2000, step: 1, def: 20, unit: "MB" },
    ],
  },
  { id: "file.join", pillar: "file", accepts: [], command: "file_join", status: "ready", options: [] },
  {
    id: "file.data",
    pillar: "file",
    accepts: ["csv", "json"],
    command: "data_convert",
    status: "ready",
    options: [{ kind: "toggle", id: "pretty", label: "opt.prettyJson", def: true }],
  },
  {
    id: "file.touch",
    pillar: "file",
    accepts: [],
    command: "file_touch",
    status: "ready",
    options: [
      { kind: "number", id: "shiftHours", label: "opt.shiftHours", min: -240, max: 240, step: 1, def: 0, unit: "h" },
    ],
  },
  {
    id: "file.unzip",
    pillar: "file",
    accepts: ["zip"],
    command: "zip_extract",
    status: "ready",
    highlight: true,
    options: [
      { kind: "text", id: "password", label: "opt.password", def: "", placeholder: "opt.zipPwHint" },
    ],
  },
  {
    id: "file.mkdirs",
    pillar: "file",
    accepts: ["txt", "csv"],
    command: "dirs_create",
    status: "ready",
    options: [],
  },
  {
    id: "file.hash",
    pillar: "file",
    accepts: [],
    command: "file_hash",
    status: "ready",
    output: "text",
    options: [
      {
        kind: "choice",
        id: "algo",
        label: "opt.hashAlgo",
        def: "sha256",
        choices: [
          { value: "sha256", label: "SHA-256" },
          { value: "blake3", label: "BLAKE3" },
        ],
      },
    ],
  },
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
