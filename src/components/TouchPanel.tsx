import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../i18n";
import { asAppErr } from "../errText";
import { burstConfetti } from "../confetti";
import { stampDone } from "../stamp";
import { ToolHead } from "./ToolHead";

/**
 * 批量改文件时间。
 *
 * 和其他工具最大的不同：它**直接改原文件**的时间属性（时间不是内容，
 * 复制一份出来改毫无意义）。正因为动了原件，就得像重命名一样：动手前
 * 先把每个文件的原时间记下来，给一条一键撤销的后路。所以单独一个面板，
 * 不走通用的「拖入 → 配置 → 执行」——那条链路承载不了撤销。
 */

const PRESETS = [-8, -1, 1, 8];

export function TouchPanel({
  onHistory,
}: {
  onHistory?: (e: { toolId: string; summary: string; outPath: string | null }) => void;
}) {
  const { t } = useI18n();
  const [paths, setPaths] = useState<string[]>([]);
  const [hours, setHours] = useState(0);
  const [undoLog, setUndoLog] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [result, setResult] = useState<{ done: number; failed: number } | null>(null);

  const addPaths = useCallback((incoming: string[]) => {
    setPaths((prev) => [...new Set([...prev, ...incoming])]);
    setResult(null);
    setUndoLog(null);
    setErr(null);
    setMsg(null);
  }, []);

  useEffect(() => {
    const un = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "drop") addPaths(e.payload.paths);
    });
    return () => {
      un.then((f) => f());
    };
  }, [addPaths]);

  const pick = async () => {
    const sel = await open({ multiple: true });
    if (Array.isArray(sel)) addPaths(sel);
    else if (typeof sel === "string") addPaths([sel]);
  };

  const apply = async () => {
    setErr(null);
    setMsg(null);
    try {
      const r = await invoke<{ done: number; failed: number; undo_log: string }>("touch_apply", {
        paths,
        shiftHours: hours,
      });
      setResult(r);
      setUndoLog(r.undo_log || null);
      if (r.done > 0) {
        onHistory?.({ toolId: "file.touch", summary: t("history.items", { n: r.done }), outPath: null });
        burstConfetti(window.innerWidth / 2, window.innerHeight * 0.82);
        stampDone();
      }
      // 时间已改，这一批不再重复操作，清空重来
      setPaths([]);
    } catch (e) {
      const ae = asAppErr(e);
      setErr(t(ae.key as never, ae.vars));
    }
  };

  const undo = async () => {
    if (!undoLog) return;
    try {
      const r = await invoke<{ restored: number; failed: number }>("touch_undo", {
        logPath: undoLog,
      });
      if (r.failed === 0) {
        setUndoLog(null);
        setResult(null);
        setMsg(t("touch.undone", { count: r.restored }));
      } else {
        // 有没还原成功的：日志还在，留着按钮让用户再撤一次
        setErr(t("touch.undonePartial", { restored: r.restored, failed: r.failed }));
      }
    } catch (e) {
      const ae = asAppErr(e);
      setErr(t(ae.key as never, ae.vars));
    }
  };

  const fileName = (p: string) => p.replace(/^.*[\\/]/, "");

  return (
    <div className="toolpage">
      <ToolHead id="file.touch" />
      <p className="lede">{t("tool.file.touch.desc")}</p>

      <button className="addbar" onClick={pick}>
        <span className="addbar__plus">＋</span>
        {paths.length === 0 ? t("touch.pick") : t("touch.picked", { count: paths.length })}
      </button>

      {/* 偏移量控制带：一个小时数 + 几个常用时区档 */}
      <div className="optbar">
        <div className="opt">
          <span className="opt__label">{t("touch.shiftLabel")}</span>
          <input
            type="number"
            value={hours}
            style={{ width: 84 }}
            onChange={(e) => setHours(Math.trunc(+e.target.value) || 0)}
          />
          <span className="opt__unit">h</span>
        </div>
        <div className="opt">
          {PRESETS.map((h) => (
            <button key={h} className="chip" aria-pressed={hours === h} onClick={() => setHours(h)}>
              {h > 0 ? `+${h}` : h}
            </button>
          ))}
          <button className="chip" aria-pressed={hours === 0} onClick={() => setHours(0)}>
            {t("touch.reset")}
          </button>
        </div>
      </div>
      <p className="lede">{t("touch.shiftHint")}</p>

      {paths.length === 0 ? (
        <div className="empty">
          <div className="empty__box">◷</div>
          <h2 className="empty__title">{t("touch.emptyTitle")}</h2>
          <p className="empty__hint">{t("touch.emptyHint")}</p>
        </div>
      ) : (
        <div className="filelist">
          {paths.map((p) => (
            <div key={p} className="renamerow">
              <span className="renamerow__old">{fileName(p)}</span>
            </div>
          ))}
        </div>
      )}

      <div className="runbar">
        <button className="go" onClick={apply} disabled={paths.length === 0 || hours === 0}>
          {t("touch.apply", { count: paths.length })}
        </button>
        {undoLog && (
          <button className="chip is-danger" onClick={undo}>
            {t("touch.undo")}
          </button>
        )}
      </div>

      {result && (
        <p className="lede">
          {t("touch.result", { done: result.done, failed: result.failed })}
        </p>
      )}

      {result && result.done > 0 && !undoLog && (
        <div className="notice">
          <span className="notice__mark">!</span>
          <span>{t("touch.noUndoLog")}</span>
        </div>
      )}

      {msg && (
        <div className="notice">
          <span className="notice__mark">✓</span>
          <span>{msg}</span>
        </div>
      )}

      {err && (
        <div className="notice notice--bad">
          <span className="notice__mark">!</span>
          <span>{err}</span>
        </div>
      )}
    </div>
  );
}
