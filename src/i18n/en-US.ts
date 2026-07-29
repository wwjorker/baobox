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
      batch: {
        name: "Batch OCR to one file",
        desc: "Read a stack of scans in one pass and merge them into a single transcript with filename headings.",
      },
      screen: { name: "Screen capture OCR", desc: "Global hotkey, drag over any part of the screen, get the text." },
    },
    pdf: {
      merge: { name: "Merge", desc: "Drop several PDFs in, order them, combine into one." },
      split: { name: "Split / extract / delete pages", desc: "Pick pages from thumbnails — what you see is what you get." },
      rotate: { name: "Rotate & reorder", desc: "Change page orientation and sequence." },
      compress: { name: "Compress", desc: "Recompress embedded images and show the before/after size." },
      "to-image": { name: "PDF to image", desc: "Export as PNG or JPG at your chosen DPI." },
      "from-image": {
        name: "Image to PDF",
        desc: "Combine several images into one PDF, page size following each image.",
      },
      "to-text": {
        name: "Extract PDF text",
        desc: "Pull plain text out of PDFs that already have a text layer. Use OCR for scans.",
      },
      decrypt: {
        name: "Unlock PDF restrictions",
        desc: "Some PDFs open fine but block printing and copying — clearing those permission restrictions needs no password. If an open password is set, you'll need to supply it. Unlocking also lets the other tools process the file.",
      },
      encrypt: {
        name: "Set PDF password",
        desc: "Add an open password to a PDF. Not built yet — see the note below.",
      },
      stamp: { name: "Page numbers & watermark", desc: "Stamp every page with a watermark and page numbers, Chinese included. Only the glyphs actually used get embedded, so a 19.7 MB font costs about 13 KB." },
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
    outputTo: "Results go to a {dir} folder next to your files. Originals are left untouched.",
    pick: "Choose files",
    openOutput: "Open output folder",
    summary: "{ok} succeeded, {fail} failed · {saved} saved in total",
    summaryClean: "All {ok} succeeded · {saved} saved in total",
    grew: "didn't get smaller",
    copy: "Copy",
    copied: "Copied",
    copyAll: "Copy all text",
    noText: "No text found",
    langs: "OCR languages: {list}",
    langsNone: "No OCR language pack installed",
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
    ocrLang: "Recognition language",
    ocrLangAuto: "Auto",
    rotate: "Rotation",
    pageNumbers: "Page numbers",
  },

  dedupe: {
    pickFolder: "Choose folders to scan",
    roots: "{count} folder(s) · {list} · click to change",
    emptyTitle: "Pick a folder to start",
    emptyHint:
      "Compares actual content, so renamed copies still match. Files under 64 KB are skipped — duplicates that small aren't worth reclaiming.",
    scan: "Scan",
    scanning: "Scanning…",
    phaseWalk: "Walking files",
    phaseQuick: "Quick compare",
    phaseFull: "Exact compare",
    phaseDone: "Collecting",
    summary: "Scanned {scanned} files, found {groups} duplicate sets, {size} reclaimable",
    skippedCloud: "skipped {count} cloud placeholders (reading them would trigger downloads)",
    groupHead: "{count} identical · {each} each · {save} reclaimable",
    deleteBtn: "Delete {count} selected ({size})",
    confirmTitle: "Delete these?",
    confirmLead: "About to delete {count} files, {size} in total.",
    confirmWipe: "Note: {count} set(s) are fully selected — no copy of that content will remain.",
    andMore: "…and {count} more not listed",
    recycleNote: "Files go to the Recycle Bin, so anything deleted by mistake can be restored.",
    confirmGo: "Delete",
  },

  status: {
    ready: "Ready",
    wip: "In progress",
    planned: "Not built yet",
    plannedHint: "This tool hasn't been built yet. The app is still at the skeleton stage — features are landing gradually.",
    highlight: "Highlight",
    encryptWhy:
      "Deliberately deferred. lopdf can decrypt but not encrypt, and a hand-rolled PDF encryption that gets it wrong hands you a file that claims to be protected while offering no real protection — worse than saying it isn't supported. Waiting on a vetted implementation.",
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
    encrypted: "This PDF is password protected. Unlock it with the Unlock PDF restrictions tool first.",
    tooLarge: "This file is beyond what Baobox can handle. Try splitting it first.",
    noPermission: "No permission to read that location. Try copying the file to your desktop first.",
    pathTooLong: "The path is too long (over the Windows 260-character limit). Move the file somewhere shallower.",
    notFound: "Can't find this file — it may have been moved or deleted.",
    pdfNoPages: "This PDF has no pages — it may be damaged.",
    pdfWrongPassword: "That password doesn't unlock this PDF. Check it and try again.",
    ocrNoLanguage:
      "No OCR language pack is installed. Add the Optical Character Recognition feature for your language under Settings → Time & language → Language & region, then try again.",
    unknown: "Failed: {detail}",
  },

  lang: { switch: "简体中文", name: "English" },
};

export default enUS;
