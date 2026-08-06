import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useI18n } from "../i18n";
import { ToolHead } from "./ToolHead";
import { noteText, type Note } from "../notes";

/**
 * 图片打码
 *
 * 在图上框选要遮掉的区域。选区用**相对比例**存储，所以同一组框
 * 能套用到整批尺寸不同的图上——批量处理同一版式的截图时省事。
 *
 * 遮挡是真正改写像素的，改完原图无法还原。网页上那种盖层黑矩形的
 * 做法，原始数据还在文件里，那不是打码。
 */

interface Region {
  x: number;
  y: number;
  w: number;
  h: number;
}
interface Preview {
  data_url: string;
  width: number;
  height: number;
}
interface Outcome {
  name: string;
  ok: boolean;
  out_path: string | null;
  note: Note | null;
}

export function RedactPanel() {
  const { t } = useI18n();
  const [paths, setPaths] = useState<string[]>([]);
  const [preview, setPreview] = useState<Preview | null>(null);
  const [regions, setRegions] = useState<Region[]>([]);
  const [drag, setDrag] = useState<Region | null>(null);
  const [mode, setMode] = useState<"pixelate" | "blackout">("pixelate");
  const [results, setResults] = useState<Outcome[] | null>(null);
  const [busy, setBusy] = useState(false);
  const startRef = useRef<{ x: number; y: number } | null>(null);
  const imgRef = useRef<HTMLImageElement>(null);

  const addPaths = async (incoming: string[]) => {
    const imgs = incoming.filter((p) =>
      ["jpg", "jpeg", "png", "webp", "bmp"].includes(p.split(".").pop()?.toLowerCase() ?? ""),
    );
    if (imgs.length === 0) return;
    const next = [...new Set([...paths, ...imgs])];
    setPaths(next);
    setResults(null);
    // 第一张作为画选区的样板
    if (!preview) {
      const p = await invoke<Preview>("image_preview", { path: imgs[0] });
      setPreview(p);
    }
  };

  useEffect(() => {
    const un = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "drop") addPaths(e.payload.paths);
    });
    return () => {
      un.then((f) => f());
    };
  });

  const rel = (clientX: number, clientY: number) => {
    const img = imgRef.current;
    if (!img) return { x: 0, y: 0 };
    const b = img.getBoundingClientRect();
    return {
      x: Math.min(1, Math.max(0, (clientX - b.left) / b.width)),
      y: Math.min(1, Math.max(0, (clientY - b.top) / b.height)),
    };
  };

  const onDown = (e: React.MouseEvent) => {
    if (!preview) return;
    startRef.current = rel(e.clientX, e.clientY);
    setDrag({ ...startRef.current, w: 0, h: 0 });
  };
  const onMove = (e: React.MouseEvent) => {
    if (!startRef.current) return;
    const n = rel(e.clientX, e.clientY);
    const s = startRef.current;
    setDrag({
      x: Math.min(s.x, n.x),
      y: Math.min(s.y, n.y),
      w: Math.abs(n.x - s.x),
      h: Math.abs(n.y - s.y),
    });
  };
  const onUp = () => {
    if (drag && drag.w > 0.005 && drag.h > 0.005) setRegions((r) => [...r, drag]);
    setDrag(null);
    startRef.current = null;
  };

  const apply = async () => {
    if (regions.length === 0 || paths.length === 0) return;
    setBusy(true);
    try {
      const r = await invoke<Outcome[]>("img_redact", { paths, regions, mode });
      setResults(r);
    } finally {
      setBusy(false);
    }
  };

  const pick = async () => {
    const sel = await open({
      multiple: true,
      filters: [{ name: "images", extensions: ["jpg", "jpeg", "png", "webp", "bmp"] }],
    });
    if (Array.isArray(sel)) addPaths(sel);
    else if (typeof sel === "string") addPaths([sel]);
  };

  const boxes = [...regions, ...(drag ? [drag] : [])];

  return (
    <>
      <ToolHead id="image.redact">
        <span className="badge is-highlight">{t("status.highlight")}</span>
      </ToolHead>
      <p className="lede">{t("tool.image.redact.desc")}</p>

      <button className="addbar" onClick={pick}>
        <span className="addbar__plus">＋</span>
        {paths.length === 0 ? t("redact.pick") : t("redact.picked", { count: paths.length })}
      </button>

      {!preview ? (
        <div className="empty">
          <div className="empty__box">▨</div>
          <h2 className="empty__title">{t("redact.emptyTitle")}</h2>
          <p className="empty__hint">{t("redact.emptyHint")}</p>
        </div>
      ) : (
        <>
          <div className="optbar">
            <div className="opt">
              <span className="opt__label">{t("opt.redactMode")}</span>
              <button
                className="chip"
                aria-pressed={mode === "pixelate"}
                onClick={() => setMode("pixelate")}
              >
                {t("opt.redactPixelate")}
              </button>
              <button
                className="chip"
                aria-pressed={mode === "blackout"}
                onClick={() => setMode("blackout")}
              >
                {t("opt.redactBlackout")}
              </button>
            </div>
            {regions.length > 0 && (
              <div className="opt">
                <span className="opt__label">{t("redact.count", { n: regions.length })}</span>
                <button className="chip" onClick={() => setRegions([])}>
                  {t("redact.clear")}
                </button>
              </div>
            )}
          </div>

          <p className="lede">{t("redact.dragHint")}</p>
          <div
            className="shotwrap"
            onMouseDown={onDown}
            onMouseMove={onMove}
            onMouseUp={onUp}
            onMouseLeave={() => startRef.current && onUp()}
          >
            <img ref={imgRef} src={preview.data_url} alt="" className="shotwrap__img" draggable={false} />
            {boxes.map((r, i) => (
              <div
                key={i}
                className="shotwrap__sel is-solid"
                style={{
                  left: `${r.x * 100}%`,
                  top: `${r.y * 100}%`,
                  width: `${r.w * 100}%`,
                  height: `${r.h * 100}%`,
                }}
              />
            ))}
          </div>
          <p className="lede">
            {t("redact.appliesTo", { count: paths.length })}
          </p>
        </>
      )}

      {results && (
        <div className="filelist">
          {results.map((r) => (
            <div key={r.name} className={`row is-${r.ok ? "done" : "failed"}`}>
              <span className="row__thumb" />
              <span className="row__name">{r.name}</span>
              <span className="row__from" />
              <span className="row__to">{r.ok ? "✓" : t("run.failed")}</span>
              <span className="pill">{noteText(r.note, t) ?? ""}</span>
            </div>
          ))}
        </div>
      )}

      <div className="runbar">
        <button
          className="go"
          onClick={apply}
          disabled={busy || regions.length === 0 || paths.length === 0}
        >
          {busy ? t("run.running") : t("redact.apply", { count: paths.length })}
        </button>
        {results?.[0]?.out_path && (
          <button className="chip" onClick={() => revealItemInDir(results[0].out_path!)}>
            {t("run.openOutput")}
          </button>
        )}
      </div>
    </>
  );
}
