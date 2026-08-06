import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../i18n";
import { asAppErr } from "../errText";
import { fmtSize } from "../useSaved";

/**
 * 文件粉碎。
 *
 * 这是整个软件里唯一会**不可逆销毁数据**的功能。安全红线要求它与普通删除
 * 彻底隔离：独立入口、独立的红色确认流程、要求手动输入确认短语、明确告知
 * 不可恢复。所以它是自己一个面板，不复用任何删除相关的组件。
 *
 * 界面这一层的确认只是第一道闸——后端还会再验一次确认短语（见 shred.rs），
 * 就算这里出 bug 也粉碎不了东西。两道都设，因为代价太高。
 */

const CONFIRM_PHRASE = "粉碎";

interface Row {
  path: string;
  name: string;
  bytes: number;
  done?: "ok" | "fail";
}

interface Meta {
  path: string;
  name: string;
  bytes: number;
  exists: boolean;
}

interface ShredOutcome {
  path: string;
  name: string;
  ok: boolean;
}

export function ShredPanel() {
  const { t } = useI18n();
  const [rows, setRows] = useState<Row[]>([]);
  const [passes, setPasses] = useState(3);
  const [confirming, setConfirming] = useState(false);
  const [typed, setTyped] = useState("");
  const [running, setRunning] = useState(false);
  const [finished, setFinished] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const addPaths = async (incoming: string[]) => {
    // 文件夹会被 stat 成不存在（它不是文件），顺带就挡掉了——
    // 粉碎只处理单个文件，绝不递归删目录
    const metas = await invoke<Meta[]>("stat_files", { paths: incoming });
    setRows((prev) => {
      const seen = new Set(prev.map((r) => r.path));
      const fresh = metas
        .filter((m) => m.exists && !seen.has(m.path))
        .map((m) => ({ path: m.path, name: m.name, bytes: m.bytes }));
      return [...prev, ...fresh];
    });
    setFinished(false);
  };

  useEffect(() => {
    const un = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "drop" && !running) addPaths(e.payload.paths);
    });
    return () => {
      un.then((f) => f());
    };
  });

  useEffect(() => {
    const un = getCurrentWebview().listen<{ outcome: ShredOutcome }>(
      "baobox://shred",
      ({ payload }) => {
        setRows((prev) =>
          prev.map((r) =>
            r.path === payload.outcome.path
              ? { ...r, done: payload.outcome.ok ? "ok" : "fail" }
              : r,
          ),
        );
      },
    );
    return () => {
      un.then((f) => f());
    };
  }, []);

  const pick = async () => {
    const sel = await open({ multiple: true });
    if (Array.isArray(sel)) addPaths(sel);
    else if (typeof sel === "string") addPaths([sel]);
  };

  const remove = (path: string) => setRows((r) => r.filter((x) => x.path !== path));

  const openConfirm = () => {
    setTyped("");
    setConfirming(true);
  };

  const doShred = async () => {
    if (typed !== CONFIRM_PHRASE) return;
    setConfirming(false);
    setRunning(true);
    setErr(null);
    try {
      await invoke<ShredOutcome[]>("shred_files", {
        paths: rows.map((r) => r.path),
        passes,
        confirm: CONFIRM_PHRASE,
      });
      setFinished(true);
    } catch (e) {
      const ae = asAppErr(e);
      setErr(t(ae.key as never, ae.vars));
    } finally {
      setRunning(false);
    }
  };

  const totalBytes = rows.reduce((n, r) => n + r.bytes, 0);
  const okCount = rows.filter((r) => r.done === "ok").length;

  return (
    <div className="toolpage">
      <h1 className="h1">
        {t("tool.file.shred.name")}
        <span className="badge is-danger">{t("shred.danger")}</span>
      </h1>
      <p className="lede">{t("tool.file.shred.desc")}</p>

      {/* 永远摆在最上面的警告——不是弹出来一次就算，是全程可见 */}
      <div className="shredwarn">
        <span className="shredwarn__mark">⚠</span>
        <div>
          <p className="shredwarn__title">{t("shred.warnTitle")}</p>
          <p className="shredwarn__body">{t("shred.warnBody")}</p>
          <p className="shredwarn__ssd">{t("shred.ssdNote")}</p>
        </div>
      </div>

      <button className="addbar" onClick={pick} disabled={running}>
        <span className="addbar__plus">＋</span>
        {rows.length === 0
          ? t("shred.pick")
          : t("shred.picked", { count: rows.length, size: fmtSize(totalBytes) })}
      </button>

      {rows.length === 0 ? (
        <div className="empty">
          <div className="empty__box">✕</div>
          <h2 className="empty__title">{t("shred.emptyTitle")}</h2>
          <p className="empty__hint">{t("shred.emptyHint")}</p>
        </div>
      ) : (
        <>
          <div className="optbar">
            <div className="opt">
              <span className="opt__label">{t("shred.passes")}</span>
              <span className="opt__value">{passes}</span>
              <span className="opt__unit">{t("shred.passUnit")}</span>
              <input
                type="range"
                min={1}
                max={7}
                step={1}
                value={passes}
                disabled={running}
                aria-label={t("shred.passes")}
                onChange={(e) => setPasses(Number(e.target.value))}
              />
            </div>
          </div>

          <div className="filelist">
            {rows.map((r) => (
              <div
                key={r.path}
                className={`row is-${r.done === "ok" ? "done" : r.done === "fail" ? "failed" : "waiting"}`}
              >
                <span className="row__thumb">
                  <span className="row__ext">
                    {r.name.split(".").pop()?.toUpperCase() ?? "?"}
                  </span>
                </span>
                <span className="row__name" title={r.path}>
                  {r.name}
                </span>
                <span className="row__from">{fmtSize(r.bytes)}</span>
                <span className="pill">
                  {r.done === "ok"
                    ? t("shred.gone")
                    : r.done === "fail"
                      ? "×"
                      : t("run.waiting")}
                </span>
                <span className="row__tools">
                  <button
                    className="rowbtn is-remove"
                    disabled={running}
                    title={t("run.remove")}
                    onClick={() => remove(r.path)}
                  >
                    ×
                  </button>
                </span>
              </div>
            ))}
          </div>

          {err && (
            <div className="notice notice--bad">
              <span className="notice__mark">!</span>
              <span>{err}</span>
            </div>
          )}

          {finished ? (
            <p className="lede">{t("shred.finished", { count: okCount })}</p>
          ) : (
            <div className="runbar">
              <button
                className="go is-danger"
                onClick={openConfirm}
                disabled={running || rows.length === 0}
              >
                {running ? t("shred.running") : t("shred.start", { count: rows.length })}
              </button>
            </div>
          )}
        </>
      )}

      {confirming && (
        <div className="confirm" onMouseDown={() => setConfirming(false)}>
          <div
            className="confirm__box"
            onMouseDown={(e) => e.stopPropagation()}
            style={{ maxWidth: 520 }}
          >
            <h2 className="confirm__title">{t("shred.confirmTitle")}</h2>
            <p className="confirm__warn">
              {t("shred.confirmWarn", { count: rows.length, size: fmtSize(totalBytes) })}
            </p>
            {/* 不可逆销毁，最后一步必须把每个文件都摆出来核对（安全红线 4）。
                容器可滚动，多少都能翻完。 */}
            <div className="confirm__list">
              {rows.map((r) => (
                <div key={r.path} className="confirm__row" title={r.path}>
                  {r.path}
                </div>
              ))}
            </div>
            <p className="confirm__lead">{t("shred.confirmType", { phrase: CONFIRM_PHRASE })}</p>
            {/* 不 autoFocus 输入框：默认焦点留给「取消」，要粉碎得先自己点进来打字。
                打字确认（红线 3）和焦点在取消（红线 4）两条都守住。 */}
            <input
              className="shredinput"
              value={typed}
              placeholder={CONFIRM_PHRASE}
              onChange={(e) => setTyped(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && typed === CONFIRM_PHRASE) doShred();
              }}
            />
            <div className="confirm__actions">
              {/* 默认焦点、也是视觉重心，仍然是「取消」——手滑的代价太大 */}
              <button className="go" autoFocus onClick={() => setConfirming(false)}>
                {t("run.cancel")}
              </button>
              <button
                className="chip is-danger"
                disabled={typed !== CONFIRM_PHRASE}
                onClick={doShred}
              >
                {t("shred.confirmGo")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
