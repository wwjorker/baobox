import { useCallback, useEffect, useState } from "react";
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
  const [result, setResult] = useState<{ out_path: string; pages: number } | null>(null);
  const [dragIdx, setDragIdx] = useState<number | null>(null);
  const [overIdx, setOverIdx] = useState<number | null>(null);

  const load = useCallback(async (path: string) => {
    setSrc(path);
    setLoading(true);
    setErr(null);
    setResult(null);
    setCards([]);
    setThumbs([]);
    try {
      const r = await invoke<PageThumbs>("pdf_page_thumbs", { path });
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
      const ae = asAppErr(e);
      setErr(t(ae.key as never, ae.vars));
      setSrc(null);
    } finally {
      setLoading(false);
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

  const reset = () =>
    setCards((cs) =>
      [...cs]
        .sort((a, b) => a.orig - b.orig)
        .map((c) => ({ ...c, rotate: 0, removed: false })),
    );

  // 拖拽重排：原生 HTML5 拖放，不引第三方库
  const onDrop = (target: number) => {
    setOverIdx(null);
    if (dragIdx === null || dragIdx === target) {
      setDragIdx(null);
      return;
    }
    setCards((prev) => {
      const next = [...prev];
      const [moved] = next.splice(dragIdx, 1);
      next.splice(target, 0, moved);
      return next;
    });
    setDragIdx(null);
  };

  const kept = cards.filter((c) => !c.removed);

  const exportPdf = async () => {
    if (!src || kept.length === 0) return;
    setBusy(true);
    setErr(null);
    setResult(null);
    try {
      const ops = kept.map((c) => ({ page: c.orig, rotate: c.rotate }));
      const r = await invoke<{ out_path: string; pages: number }>("pdf_arrange", {
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

      <button className="addbar" onClick={pick}>
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

          <div className="pagegrid">
            {cards.map((c, i) => (
              <div
                key={c.key}
                className={`pagecard${c.removed ? " is-removed" : ""}${
                  overIdx === i ? " is-over" : ""
                }`}
                draggable
                onDragStart={() => setDragIdx(i)}
                onDragOver={(e) => {
                  e.preventDefault();
                  if (overIdx !== i) setOverIdx(i);
                }}
                onDragLeave={() => setOverIdx((o) => (o === i ? null : o))}
                onDrop={() => onDrop(i)}
                onDragEnd={() => {
                  setDragIdx(null);
                  setOverIdx(null);
                }}
              >
                <div className="pagecard__thumb">
                  <img
                    src={thumbs[c.orig - 1]}
                    alt={`page ${c.orig}`}
                    style={{ transform: `rotate(${c.rotate}deg)` }}
                    draggable={false}
                  />
                  {c.removed && <span className="pagecard__x">{t("pdfpages.removed")}</span>}
                </div>
                <div className="pagecard__bar">
                  <span className="pagecard__no">{c.orig}</span>
                  <button
                    className="pagecard__btn"
                    title={t("pdfpages.rotate")}
                    onClick={() => rotateCard(c.key)}
                  >
                    ↻
                  </button>
                  <button
                    className="pagecard__btn"
                    title={c.removed ? t("pdfpages.restore") : t("pdfpages.remove")}
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
              <span>{t("pdfpages.done", { pages: result.pages })}</span>
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
