import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useI18n } from "../i18n";
import { asAppErr } from "../errText";

/**
 * 截图取字
 *
 * 和其他工具最大的不同：源头不是文件，是屏幕上的一块像素。
 *
 * 抓屏前先把自己最小化——否则截到的是 Baobox 自己的窗口，
 * 而用户想取的字正被它挡着。抓完再恢复。
 */

interface ScreenShot {
  data_url: string;
  origin_x: number;
  origin_y: number;
  width: number;
  height: number;
}

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export function ScreenOcrPanel({ autoStart }: { autoStart?: number }) {
  const { t } = useI18n();
  const [shot, setShot] = useState<ScreenShot | null>(null);
  const [rect, setRect] = useState<Rect | null>(null);
  const [dragging, setDragging] = useState(false);
  const [text, setText] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const startRef = useRef<{ x: number; y: number } | null>(null);
  const imgRef = useRef<HTMLImageElement>(null);

  const capture = useCallback(async () => {
    setBusy(true);
    setText(null);
    setRect(null);
    setErr(null);
    const win = getCurrentWindow();
    try {
      // 让自己让开，否则截到的就是这个窗口本身
      await win.minimize();
      await new Promise((r) => setTimeout(r, 320));
      const s = await invoke<ScreenShot>("capture_screen");
      setShot(s);
    } catch (e) {
      const ae = asAppErr(e);
      setErr(t(ae.key as never, ae.vars));
    } finally {
      await win.unminimize();
      // Tauri 的 setFocus() 在 Windows 上常常抢不回前台，窗口会卡在
      // 别人后面。走 Win32 的正规流程才可靠。
      await invoke("restore_and_focus").catch(() => win.setFocus());
      setBusy(false);
    }
  }, []);

  // 全局热键触发时直接开抓，用户按完 Ctrl+Shift+S 不该还要再点一下
  useEffect(() => {
    if (autoStart) capture();
  }, [autoStart, capture]);

  // 把界面上的坐标换算回屏幕的物理像素
  const toScreen = (clientX: number, clientY: number) => {
    const img = imgRef.current;
    if (!img || !shot) return { x: 0, y: 0 };
    const b = img.getBoundingClientRect();
    const scale = shot.width / b.width;
    return {
      x: Math.round((clientX - b.left) * scale),
      y: Math.round((clientY - b.top) * scale),
    };
  };

  const onDown = (e: React.MouseEvent) => {
    if (!shot) return;
    startRef.current = toScreen(e.clientX, e.clientY);
    setRect(null);
    setText(null);
    setDragging(true);
  };

  const onMove = (e: React.MouseEvent) => {
    if (!dragging || !startRef.current) return;
    const now = toScreen(e.clientX, e.clientY);
    const s = startRef.current;
    setRect({
      x: Math.min(s.x, now.x),
      y: Math.min(s.y, now.y),
      w: Math.abs(now.x - s.x),
      h: Math.abs(now.y - s.y),
    });
  };

  const onUp = async () => {
    setDragging(false);
    if (!rect || rect.w < 4 || rect.h < 4) return;
    setBusy(true);
    try {
      const got = await invoke<string>("ocr_region", {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
        lang: null,
      });
      setText(got);
    } catch (e) {
      const ae = asAppErr(e);
      setErr(t(ae.key as never, ae.vars));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setShot(null);
        setRect(null);
        setText(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const copy = async () => {
    if (!text) return;
    await navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1600);
  };

  // 选框在界面上的位置（要从物理像素换算回显示尺寸）
  const overlayRect = (() => {
    const img = imgRef.current;
    if (!img || !shot || !rect) return null;
    const b = img.getBoundingClientRect();
    const k = b.width / shot.width;
    return {
      left: rect.x * k,
      top: rect.y * k,
      width: rect.w * k,
      height: rect.h * k,
    };
  })();

  return (
    <>
      <h1 className="h1">
        {t("tool.ocr.screen.name")}
        <span className="badge is-highlight">{t("status.highlight")}</span>
      </h1>
      <p className="lede">{t("tool.ocr.screen.desc")}</p>

      {err && (
        <div className="notice notice--bad">
          <span className="notice__mark">!</span>
          <span>{err}</span>
        </div>
      )}

      {!shot ? (
        <div className="empty">
          <div className="empty__box">▣</div>
          <h2 className="empty__title">{t("screen.emptyTitle")}</h2>
          <p className="empty__hint">{t("screen.emptyHint")}</p>
        </div>
      ) : (
        <>
          <p className="lede">{t("screen.dragHint")}</p>
          <div
            className="shotwrap"
            onMouseDown={onDown}
            onMouseMove={onMove}
            onMouseUp={onUp}
            onMouseLeave={() => dragging && onUp()}
          >
            <img ref={imgRef} src={shot.data_url} alt="" className="shotwrap__img" draggable={false} />
            {overlayRect && (
              <div
                className="shotwrap__sel"
                style={{
                  left: overlayRect.left,
                  top: overlayRect.top,
                  width: overlayRect.width,
                  height: overlayRect.height,
                }}
              />
            )}
          </div>
          {rect && (
            <p className="lede">
              {t("screen.selected", { w: rect.w, h: rect.h })}
            </p>
          )}
        </>
      )}

      {text !== null && (
        <div className="textout" style={{ marginTop: 4 }}>
          <pre className="textout__body">{text.trim() || t("run.noText")}</pre>
          {text.trim() && (
            <button className="chip textout__copy" onClick={copy}>
              {copied ? t("run.copied") : t("run.copy")}
            </button>
          )}
        </div>
      )}

      <div className="runbar">
        <button className="go" onClick={capture} disabled={busy}>
          {busy ? t("screen.working") : shot ? t("screen.again") : t("screen.start")}
        </button>
      </div>
    </>
  );
}
