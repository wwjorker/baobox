import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useI18n } from "../i18n";
import { asAppErr } from "../errText";

/**
 * PDF 页面可视化整理：拆分 / 提取 / 删页 / 重排 / 逐页旋转，一处做完。
 *
 * 这几件事本质是同一件：从原文档里挑出若干页、按新顺序、各带一个旋转。
 * 所以界面就是一排页面缩略图——勾掉不要的、拖动换顺序、点一下转向——
 * 最后把这份「保留哪些、什么次序、各转多少」的清单交给后端 pdf_arrange。
 *
 * 走专用面板而不是通用框架：通用框架是「一批文件、一组参数」，
 * 这里是「一份文件、逐页摆布」，是另一种交互。
 */

interface PageThumbs {
  count: number;
  thumbs: string[];
}

interface Card {
  /** 稳定 key，用原始页序当种子，重排时不复用会导致缩略图闪 */
  key: number;
  /** 原文档里的页码，从 1 开始 */
  orig: number;
  rotate: number;
  removed: boolean;
}

export function PdfPagesPanel() {
  const { t } = useI18n();
  const [src, setSrc] = useState<string | null>(null);
  const [thumbs, setThumbs] = useState<string[]>([]);
  const [cards, setCards] = useState<Card[]>([]);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [result, setResult] = useState<{ out_path: string; pages: number; dropped: boolean } | null>(
    null,
  );
  const [dragIdx, setDragIdx] = useState<number | null>(null);
  const [overIdx, setOverIdx] = useState<number | null>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  // 加载代号：换 PDF 时递增，只有最新那次的结果能落地。
  // 否则先点 A 后点 B、A 后返回，会出现「src 是 B、缩略图却是 A」，
  // 导出就会把按 A 排的页码用到 B 上——产出和预览不符的 PDF。
  const loadGen = useRef(0);

  const load = useCallback(async (path: string) => {
    const gen = ++loadGen.current;
    setSrc(path);
    setLoading(true);
    setErr(null);
    setResult(null);
    setCards([]);
    setThumbs([]);
    try {
      const r = await invoke<PageThumbs>("pdf_page_thumbs", { path });
      if (gen !== loadGen.current) return; // 已经有更新的加载了，丢弃这次
      setThumbs(r.thumbs);
      setCards(
        Array.from({ length: r.count }, (_, i) => ({
          key: i,
          orig: i + 1,
          rotate: 0,
          removed: false,
        })),
      );
    } catch (e) {
      if (gen !== loadGen.current) return;
      const ae = asAppErr(e);
      setErr(t(ae.key as never, ae.vars));
      setSrc(null);
    } finally {
      if (gen === loadGen.current) setLoading(false);
    }
  }, [t]);

  // 只收第一份 PDF——可视化整理是对着一份文档逐页摆布，多份没有意义
  const takeFirstPdf = useCallback(
    (paths: string[]) => {
      const pdf = paths.find((p) => p.toLowerCase().endsWith(".pdf"));
      if (pdf) load(pdf);
    },
    [load],
  );

  useEffect(() => {
    const un = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "drop") takeFirstPdf(e.payload.paths);
    });
    return () => {
      un.then((f) => f());
    };
  }, [takeFirstPdf]);

  const pick = async () => {
    const sel = await open({ multiple: false, filters: [{ name: "PDF", extensions: ["pdf"] }] });
    if (typeof sel === "string") load(sel);
  };

  const rotateCard = (key: number) =>
    setCards((cs) => cs.map((c) => (c.key === key ? { ...c, rotate: (c.rotate + 90) % 360 } : c)));

  const toggleRemove = (key: number) =>
    setCards((cs) => cs.map((c) => (c.key === key ? { ...c, removed: !c.removed } : c)));

  const rotateAll = () =>
    setCards((cs) => cs.map((c) => (c.removed ? c : { ...c, rotate: (c.rotate + 90) % 360 })));

  // 键盘/鼠标都能用的前移后移，拖拽之外的另一条路（拖拽对键盘用户不可达）
  const move = (i: number, delta: number) =>
    setCards((prev) => {
      const j = i + delta;
      if (j < 0 || j >= prev.length) return prev;
      const next = [...prev];
      [next[i], next[j]] = [next[j], next[i]];
      return next;
    });

  const reset = () =>
    setCards((cs) =>
      [...cs]
        .sort((a, b) => a.orig - b.orig)
        .map((c) => ({ ...c, rotate: 0, removed: false })),
    );

  // 拖拽重排：用 Pointer Events 自己实现，不用原生 HTML5 拖放。
  // Tauri 开着「拖文件进来」（dragDropEnabled）时会在系统层截走 HTML5 拖放，
  // WebView 里的 draggable 根本不触发——这是之前拖不动的原因。
  // 用 setPointerCapture：光标移出窗口也照样收到事件，不会卡在拖动态；
  // 落点用 elementFromPoint 直接命中，不必每帧遍历所有卡片；再用 rAF 限频。
  const rafRef = useRef(0);

  const cardIndexAt = (x: number, y: number): number | null => {
    const grid = gridRef.current;
    if (!grid) return null;
    const card = (document.elementFromPoint(x, y) as HTMLElement | null)?.closest(".pagecard");
    if (!card) return null;
    const idx = Array.prototype.indexOf.call(grid.children, card);
    return idx >= 0 ? idx : null;
  };

  // 从卡片主体按下起拖；点到按钮上不算（那是旋转/删除/移动）
  const startDrag = (e: React.PointerEvent, i: number) => {
    if ((e.target as HTMLElement).closest("button")) return;
    e.preventDefault();
    try {
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    } catch {
      /* 某些环境不支持捕获，退化为普通拖动即可 */
    }
    setDragIdx(i);
    setOverIdx(i);
  };

  const onDragMove = (e: React.PointerEvent) => {
    const { clientX: x, clientY: y } = e;
    cancelAnimationFrame(rafRef.current);
    rafRef.current = requestAnimationFrame(() => setOverIdx(cardIndexAt(x, y)));
  };

  const endDrag = (e: React.PointerEvent, from: number) => {
    cancelAnimationFrame(rafRef.current);
    const to = cardIndexAt(e.clientX, e.clientY);
    setDragIdx(null);
    setOverIdx(null);
    if (to === null || to === from) return;
    setCards((prev) => {
      const next = [...prev];
      const [moved] = next.splice(from, 1);
      next.splice(to, 0, moved);
      return next;
    });
  };

  const kept = cards.filter((c) => !c.removed);

  const exportPdf = async () => {
    if (!src || kept.length === 0) return;
    setBusy(true);
    setErr(null);
    setResult(null);
    try {
      const ops = kept.map((c) => ({ page: c.orig, rotate: c.rotate }));
      const r = await invoke<{ out_path: string; pages: number; dropped: boolean }>("pdf_arrange", {
        path: src,
        ops,
      });
      setResult(r);
    } catch (e) {
      const ae = asAppErr(e);
      setErr(t(ae.key as never, ae.vars));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <h1 className="h1">{t("tool.pdf.split.name")}</h1>
      <p className="lede">{t("pdfpages.desc")}</p>

      <button className="addbar" onClick={pick} disabled={loading}>
        <span className="addbar__plus">＋</span>
        {src ? t("pdfpages.pickAnother") : t("pdfpages.pick")}
      </button>

      {err && (
        <div className="notice notice--bad">
          <span className="notice__mark">!</span>
          <span>{err}</span>
        </div>
      )}

      {loading ? (
        <div className="empty">
          <div className="empty__box is-spin">◴</div>
          <h2 className="empty__title">{t("pdfpages.loading")}</h2>
          <p className="empty__hint">{t("pdfpages.loadingHint")}</p>
        </div>
      ) : cards.length === 0 ? (
        <div className="empty">
          <div className="empty__box">▤</div>
          <h2 className="empty__title">{t("pdfpages.emptyTitle")}</h2>
          <p className="empty__hint">{t("pdfpages.emptyHint")}</p>
        </div>
      ) : (
        <>
          <div className="optbar">
            <span className="opt__label">
              {t("pdfpages.kept", { kept: kept.length, total: cards.length })}
            </span>
            <div className="opt">
              <button className="chip" onClick={rotateAll}>
                {t("pdfpages.rotateAll")}
              </button>
              <button className="chip" onClick={reset}>
                {t("pdfpages.reset")}
              </button>
            </div>
            <span className="opt__label pdfpages__hint">{t("pdfpages.dragHint")}</span>
          </div>

          <div className="pagegrid" ref={gridRef}>
            {cards.map((c, i) => (
              <div
                key={c.key}
                className={`pagecard${c.removed ? " is-removed" : ""}${
                  overIdx === i && dragIdx !== i ? " is-over" : ""
                }${dragIdx === i ? " is-dragging" : ""}`}
                onPointerDown={(e) => startDrag(e, i)}
                onPointerMove={dragIdx === i ? onDragMove : undefined}
                onPointerUp={dragIdx === i ? (e) => endDrag(e, i) : undefined}
                onPointerCancel={dragIdx === i ? (e) => endDrag(e, i) : undefined}
              >
                <div className="pagecard__thumb">
                  <img
                    src={thumbs[c.orig - 1]}
                    alt={`page ${c.orig}`}
                    // 转 90/270 后长边会顶出固定高的框，缩一点避免被裁
                    style={{
                      transform: `rotate(${c.rotate}deg)${c.rotate % 180 !== 0 ? " scale(0.72)" : ""}`,
                    }}
                    draggable={false}
                  />
                  {c.removed && <span className="pagecard__x">{t("pdfpages.removed")}</span>}
                </div>
                <div className="pagecard__bar">
                  <button
                    className="pagecard__btn"
                    title={t("pdfpages.moveLeft")}
                    aria-label={t("pdfpages.moveLeft")}
                    disabled={i === 0}
                    onClick={() => move(i, -1)}
                  >
                    ◀
                  </button>
                  <button
                    className="pagecard__btn"
                    title={t("pdfpages.moveRight")}
                    aria-label={t("pdfpages.moveRight")}
                    disabled={i === cards.length - 1}
                    onClick={() => move(i, 1)}
                  >
                    ▶
                  </button>
                  <span className="pagecard__no">{c.orig}</span>
                  <button
                    className="pagecard__btn"
                    title={t("pdfpages.rotate")}
                    aria-label={t("pdfpages.rotate")}
                    onClick={() => rotateCard(c.key)}
                  >
                    ↻
                  </button>
                  <button
                    className="pagecard__btn"
                    title={c.removed ? t("pdfpages.restore") : t("pdfpages.remove")}
                    aria-label={c.removed ? t("pdfpages.restore") : t("pdfpages.remove")}
                    onClick={() => toggleRemove(c.key)}
                  >
                    {c.removed ? "↩" : "✕"}
                  </button>
                </div>
              </div>
            ))}
          </div>

          <div className="runbar">
            <button className="go" onClick={exportPdf} disabled={busy || kept.length === 0}>
              {busy ? t("pdfpages.exporting") : t("pdfpages.export", { count: kept.length })}
            </button>
          </div>

          {result && (
            <div className="notice">
              <span className="notice__mark">✓</span>
              <span>
                {t("pdfpages.done", { pages: result.pages })}
                {result.dropped && ` ${t("pdfpages.dropped")}`}
              </span>
              <button className="chip" onClick={() => revealItemInDir(result.out_path)}>
                {t("run.openOutput")}
              </button>
            </div>
          )}
        </>
      )}
    </>
  );
}
