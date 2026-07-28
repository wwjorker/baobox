import type zhCN from "./zh-CN";

/** English copy. Must stay structurally identical to zh-CN.ts — the type below enforces it. */
const enUS: typeof zhCN = {
  app: {
    name: "Baobox",
    tagline: "Local file workbench",
    offline: "Fully offline · no network requests made",
    saved: "Saved",
    search: "Search {count} tools",
    minimize: "Minimize",
    maximize: "Maximize",
    close: "Close",
  },

  pillar: {
    image: "Images",
    ocr: "OCR",
    pdf: "PDF",
    file: "Files",
    recent: "Recent",
    tools: "Tools",
  },

  tool: {
    image: {
      "compress-target": {
        name: "Compress to a target size",
        desc: "Upload limit of 500 KB? Drop files in and Baobox closes in on the target size with the least quality loss. Originals are never touched.",
      },
      compress: { name: "Batch compress", desc: "Compress a whole folder at once. No count or size caps." },
      convert: { name: "Convert format", desc: "Batch conversion between JPG, PNG and WebP." },
      resize: { name: "Batch resize", desc: "By percentage, by long edge, or to exact dimensions." },
      "strip-exif": {
        name: "Strip EXIF privacy data",
        desc: "Photos carry GPS coordinates and device details. Clear them before sharing.",
      },
      watermark: { name: "Add watermark", desc: "Text or image watermark with adjustable position and opacity." },
      redact: { name: "Redact", desc: "Pixelate or black out ID numbers and phone numbers." },
    },
    ocr: {
      image: { name: "Extract text from image", desc: "Uses the built-in Windows engine. Offline and free." },
      batch: { name: "Batch OCR", desc: "Read a whole folder and export the text." },
      screen: { name: "Screen capture OCR", desc: "Global hotkey, drag over any part of the screen, get the text." },
    },
    pdf: {
      merge: { name: "Merge", desc: "Drop several PDFs in, order them, combine into one." },
      split: { name: "Split / extract / delete pages", desc: "Pick pages from thumbnails — what you see is what you get." },
      rotate: { name: "Rotate & reorder", desc: "Change page orientation and sequence." },
      compress: { name: "Compress", desc: "Recompress embedded images and show the before/after size." },
      "to-image": { name: "PDF to image", desc: "Export as PNG or JPG at your chosen DPI." },
      "from-image": { name: "Image to PDF", desc: "Combine several images into one PDF." },
      encrypt: { name: "Encrypt / decrypt", desc: "Set an open password, or remove one you know." },
      stamp: { name: "Page numbers & watermark", desc: "Add page numbers, text or an image watermark." },
    },
    file: {
      rename: { name: "Batch rename", desc: "Stackable rules, live preview, and one-click undo." },
      dedupe: { name: "Find duplicates", desc: "Compares actual content, not just file names." },
    },
  },

  run: {
    dropHere: "Drop files here",
    dropHint: "or click to browse · no count or size limits",
    ready: "{count} files ready · {size} total",
    addMore: "{count} ready · {size} · click to add more",
    start: "Start",
    running: "Working…",
    done: "Done ✓ Run again",
    cancel: "Cancel",
    waiting: "Waiting",
    failed: "Failed",
    emptyTitle: "Drop your files in",
    emptyHint: "Supports {formats}. Any number of files, any size.",
    outputTo: "Results go to {dir}. Your originals are left untouched.",
  },

  opt: {
    targetSize: "Max size each",
    format: "Output format",
    formatKeep: "Keep original",
    quality: "Quality",
    longEdge: "Long edge",
    keepOrientation: "Keep orientation data",
    watermarkText: "Watermark text",
    watermarkPlaceholder: "e.g. Internal use only",
    opacity: "Opacity",
    redactMode: "Redaction style",
    redactPixelate: "Pixelate",
    redactBlackout: "Black out",
    dpi: "Resolution",
    password: "Password",
  },

  status: {
    ready: "Ready",
    wip: "In progress",
    planned: "Not built yet",
    plannedHint: "This tool hasn't been built yet. The app is still at the skeleton stage — features are landing gradually.",
    highlight: "Highlight",
  },

  result: {
    from: "Before",
    to: "After",
    saved: "Saved",
    skipReason: "Skip and continue",
    showInFolder: "Show in folder",
  },

  err: {
    decode: "Can't read this file — it may be damaged or not actually a {format} file.",
    encrypted: "This PDF is password protected. Remove the password with the Encrypt / decrypt tool first.",
    tooLarge: "This file is beyond what Baobox can handle. Try splitting it first.",
    noPermission: "No permission to read that location. Try copying the file to your desktop first.",
    pathTooLong: "The path is too long (over the Windows 260-character limit). Move the file somewhere shallower.",
    unknown: "Failed: {detail}",
  },

  lang: { switch: "简体中文", name: "English" },
};

export default enUS;
