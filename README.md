# Baobox 百宝箱

**A local file toolkit for Windows. No uploads, no limits, no network.**

> ## 🚧 Work in progress — not usable yet
>
> There is **no release to download**. The project was scaffolded in July 2026 and is
> in the technical-validation stage. Everything under "Planned features" is planned,
> **not built**. Star or watch if you want to hear when v1.0 ships.

---

## Why

Online tools like iLovePDF, Smallpdf and TinyPNG are used by millions, and they all
share the same four problems:

- Your files get uploaded to someone else's server
- Free tiers cap file size, batch count, and daily usage
- Nothing works without a connection
- Ads and upsell prompts everywhere

The local alternatives aren't great either. [Stirling-PDF](https://github.com/Stirling-Tools/Stirling-PDF)
is excellent but **requires Docker**, which rules it out for most people who just want
to sign one PDF. Others (FileOptimizer, IrfanView) have interfaces from another decade,
or you end up installing a separate program for every single task.

**Baobox is a single double-clickable app.** No Docker, no Java, no Python.
Your files never leave the machine because the app never opens a socket.

## Planned features

### v1.0 — 20 tools

| Area | Tools |
|---|---|
| **Images** | Batch compress · **Compress to a target file size** · Format conversion (jpg/png/webp) · Batch resize · **Strip EXIF privacy data** · Watermark · **Redact / pixelate** |
| **OCR** | Extract text from images · Batch OCR · Screen-capture OCR (global hotkey) |
| **PDF** | Merge · Split / extract / delete pages · Rotate & reorder · Compress · PDF→image · image→PDF · Encrypt / decrypt · Page numbers & watermark |
| **Files** | Batch rename (rule chain, live preview, undo) · Duplicate file finder |

### Later — ~60 more

PDF cropping, N-up imposition, redaction, flattening, signatures, metadata scrubbing,
compare, searchable-PDF OCR · AVIF/ICO/HEIC, colour adjustment, long-image stitching,
9-grid slicing, GIF tools, reusable presets · archive extraction, QR codes,
CSV/Excel/JSON conversion · GBK↔UTF-8 mojibake repair, traditional↔simplified Chinese.

## Non-negotiable safety rules

These are enforced throughout the codebase, not aspirations:

1. **Original files are never overwritten.** Output always goes to a new directory.
2. **Deletion always goes to the Recycle Bin.** Never a permanent unlink.
3. **Zero network capability.** No HTTP is compiled in; CSP blocks all remote loads.
   Pull the ethernet cable and everything still works.
4. **Redaction really removes text** from the content stream rather than drawing a
   black box over it. If a document can't be handled safely, Baobox refuses the job
   instead of giving you a false sense of security.

## Tech stack

Tauri v2 · Rust · React · TypeScript · Vite

Windows only, deliberately — OCR uses the built-in `Windows.Media.Ocr` engine, which
is faster and more accurate than Tesseract while adding **zero bytes** to the download.

## Validated so far

The riskiest parts were tested before writing any feature code:

| Risk | Result |
|---|---|
| `mozjpeg` needs a C/asm toolchain | ✅ Compiles — 2m36s cold build |
| WinRT OCR quality and speed | ✅ 77 ms per image, 4/4 keywords, mixed CJK/Latin |
| CJK text comes back space-separated | ✅ Fixed — spaces dropped only between CJK characters |
| Chinese fonts must be embedded in PDFs, but the font file is 18.8 MB and not redistributable | ✅ Subsetting works — 30 glyphs ≈ 4.5 KB of outline data |
| Office COM for PDF→Word | ⚠️ **Blocked.** Word's PDF-reflow dialog hangs headless calls, and suppressing it requires writing to the user's Word registry settings. Deferred to a later opt-in feature. |

## Development

```bash
npm install
npm run tauri dev
```

Requires Rust (MSVC toolchain), Node, and the Visual Studio C++ build tools.

## Licence

MIT

---

# 中文

**一个 Windows 本地文件工具箱。不上传、无限制、不联网。**

> ## 🚧 开发中 —— 目前还不能用
>
> **没有可下载的版本。** 项目 2026 年 7 月建立，正处在技术验证阶段。
> 下面「计划功能」里的东西**都还没做出来**。想知道 v1.0 什么时候发布，可以点 Star 或 Watch。

## 为什么做这个

iLovePDF、Smallpdf、TinyPNG 这类在线工具用户量巨大，但有四个共同硬伤：文件必须上传到别人的服务器、
免费版限制体积和数量、断网就废、广告和诱导付费。

本地替代品也不理想。[Stirling-PDF](https://github.com/Stirling-Tools/Stirling-PDF) 功能强大但**必须跑 Docker**，
对只想签一份 PDF 的普通人来说门槛太高；其余的要么界面停留在十几年前，要么一个功能得装一个软件。

**Baobox 是一个双击就能用的程序。** 不需要 Docker、Java 或 Python。
你的文件不会离开这台电脑——因为这个程序压根不会打开网络连接。

## 三个差异化的点

- **OCR 免费**。iLovePDF 和 Smallpdf 都把 OCR 锁在付费订阅里（约 $4/月）。
  Baobox 用 Windows 系统内置的 OCR 引擎，离线、免费，且比 Tesseract 更快更准。
- **压到指定体积**。网站限制上传 500KB 是高频刚需，而 TinyPNG 只能调质量档位，做不到精确控制。
- **中文用户需要的功能**：GBK/UTF-8 乱码修复、聊天记录长图拼接、九宫格切图、简繁转换——欧美产品普遍不做。

## 安全底线

这几条是代码层面的硬约束，不是口号：

1. **绝不覆盖原文件**，处理结果一律写入新目录
2. **删除只进回收站**，永不永久删除
3. **零网络能力**，断网可完整运行
4. **涂黑密文是真的从内容流里删掉文字**，而不是盖个黑框。做不到安全处理的文档会直接拒绝，
   而不是给你一个虚假的安全感

## 授权

MIT
