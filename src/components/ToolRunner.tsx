import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useI18n, type TVars } from "../i18n";
import { fmtSize } from "../useSaved";
import type { OptionDef, ToolDef } from "../tools/registry";

/**
 * 统一处理流框架：拖入 → 配置 → 执行 → 结果
 *
 * 全部工具共用这一个组件，差异由 registry 的声明描述驱动，
 * 所以新增工具不需要再写一遍界面。
 *
 * 三个状态都必须存在，缺一个就是设计稿式的自欺：
 *   · 空状态 —— 首次打开的第一印象
 *   · 处理中 —— 实时进度，逐个文件落地
 *   · 失败态 —— 真实使用一定会有损坏 / 加密 / 格式不支持的文件
 */

interface AppErr {
  key: string;
  vars: Record<string, string>;
  detail: string;
}

interface Outcome {
  path: string;
  name: string;
  ok: boolean;
  in_bytes: number;
  out_bytes: number;
  out_path: string | null;
  note: string | null;
  error: AppErr | null;
}

interface Row {
  path: string;
  name: string;
  bytes: number;
  /** 文件在磁盘上不存在（选错了、被移走了） */
  missing?: boolean;
  outcome?: Outcome;
}

interface FileMeta {
  path: string;
  name: string;
  bytes: number;
  exists: boolean;
}

export function ToolRunner({
  tool,
  onSaved,
}: {
  tool: ToolDef;
  onSaved: (bytes: number) => void;
}) {
  const { t } = useI18n();
  const [rows, setRows] = useState<Row[]>([]);
  const [running, setRunning] = useState(false);
  const [doneCount, setDoneCount] = useState(0);
  const [over, setOver] = useState(false);
  const [values, setValues] = useState<Record<string, string | number | boolean>>(() =>
    Object.fromEntries(tool.options.map((o) => [o.id, o.def])),
  );
  const outputDirRef = useRef<string | null>(null);

  const notReady = tool.status !== "ready";
  const accepts = tool.accepts.length ? tool.accepts : ["*"];
  const acceptsLabel = accepts.map((e) => e.toUpperCase()).join(" · ");

  const addPaths = useCallback(
    async (paths: string[]) => {
      const allowed = tool.accepts.length
        ? paths.filter((p) => tool.accepts.includes(p.split(".").pop()?.toLowerCase() ?? ""))
        : paths;
      if (allowed.length === 0) return;

      // 先问后端要真实体积，别让列表显示一排「0 B」
      const metas = await invoke<FileMeta[]>("stat_files", { paths: allowed });
      setRows((prev) => {
        const seen = new Set(prev.map((r) => r.path));
        const fresh = metas
          .filter((m) => !seen.has(m.path))
          .map((m) => ({
            path: m.path,
            name: m.name,
            bytes: m.bytes,
            missing: !m.exists,
          }));
        return [...prev, ...fresh];
      });
    },
    [tool.accepts],
  );

  // 系统级拖放。浏览器的 drop 事件在 Tauri 里拿不到真实路径，必须用这个。
  useEffect(() => {
    const un = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "over") setOver(true);
      else if (e.payload.type === "drop") {
        setOver(false);
        addPaths(e.payload.paths);
      } else setOver(false);
    });
    return () => {
      un.then((f) => f());
    };
  }, [addPaths]);

  const pick = async () => {
    const sel = await open({
      multiple: true,
      filters: tool.accepts.length ? [{ name: "files", extensions: tool.accepts }] : undefined,
    });
    if (Array.isArray(sel)) addPaths(sel);
    else if (typeof sel === "string") addPaths([sel]);
  };

  const run = async () => {
    if (running || rows.length === 0) return;
    setRunning(true);
    setDoneCount(0);
    setRows((prev) => prev.map((r) => ({ ...r, outcome: undefined })));

    // 逐个文件上报，中途失败也保留已完成的部分
    const un = await getCurrentWebview().listen<{ index: number; total: number; outcome: Outcome }>(
      "baobox://progress",
      ({ payload }) => {
        setRows((prev) =>
          prev.map((r) => (r.path === payload.outcome.path ? { ...r, outcome: payload.outcome } : r)),
        );
        setDoneCount(payload.index + 1);
      },
    );

    try {
      const args: Record<string, unknown> = { paths: rows.map((r) => r.path), ...values };
      const results = await invoke<Outcome[]>(tool.command, args);
      const byPath = new Map(results.map((o) => [o.path, o]));
      setRows((prev) => prev.map((r) => ({ ...r, outcome: byPath.get(r.path) ?? r.outcome })));
      outputDirRef.current = results.find((r) => r.out_path)?.out_path ?? null;
      onSaved(
        results.reduce((n, o) => n + (o.ok ? Math.max(0, o.in_bytes - o.out_bytes) : 0), 0),
      );
    } finally {
      un();
      setRunning(false);
    }
  };

  const summary = useMemo(() => {
    const done = rows.filter((r) => r.outcome);
    if (done.length === 0) return null;
    const ok = done.filter((r) => r.outcome!.ok).length;
    const fail = done.length - ok;
    const saved = done.reduce(
      (n, r) => n + Math.max(0, r.outcome!.in_bytes - r.outcome!.out_bytes),
      0,
    );
    const vars: TVars = { ok, fail, saved: fmtSize(saved) };
    return fail > 0 ? t("run.summary", vars) : t("run.summaryClean", vars);
  }, [rows, t]);

  const set = (id: string, v: string | number | boolean) =>
    setValues((prev) => ({ ...prev, [id]: v }));

  const pct = rows.length ? Math.round((doneCount / rows.length) * 100) : 0;

  return (
    <>
      <div className="crumb">
        {t(`pillar.${tool.pillar}` as never)} <span>›</span>{" "}
        <b>{t(`tool.${tool.id}.name` as never)}</b>
      </div>

      <h1 className="h1">
        {t(`tool.${tool.id}.name` as never)}
        {tool.highlight && <span className="badge is-highlight">{t("status.highlight")}</span>}
        {notReady && <span className="badge">{t(`status.${tool.status}` as never)}</span>}
      </h1>
      <p className="lede">{t(`tool.${tool.id}.desc` as never)}</p>

      {notReady && (
        <div className="notice">
          <span className="notice__mark">!</span>
          <span>{t("status.plannedHint")}</span>
        </div>
      )}

      {rows.length === 0 ? (
        <div className="empty">
          <div className="empty__box">＋</div>
          <h2 className="empty__title">{t("run.emptyTitle")}</h2>
          <p className="empty__hint">{t("run.emptyHint", { formats: acceptsLabel })}</p>
          <button
            className={`dropzone${over ? " is-over" : ""}`}
            style={{ maxWidth: 420 }}
            onClick={pick}
          >
            <span className="dropzone__title">{t("run.dropHere")}</span>
            <span className="dropzone__hint">{t("run.dropHint")}</span>
          </button>
        </div>
      ) : (
        <>
          <button className="addbar" onClick={pick}>
            <span className="addbar__plus">＋</span>
            {t("run.addMore", {
              count: rows.length,
              size: fmtSize(rows.reduce((n, r) => n + (r.outcome?.in_bytes ?? r.bytes), 0)),
            })}
          </button>

          {tool.options.length > 0 && (
            <div className="optbar">
              {tool.options.map((o) => (
                <OptionControl
                  key={o.id}
                  def={o}
                  value={values[o.id]}
                  onChange={(v) => set(o.id, v)}
                />
              ))}
            </div>
          )}

          <div className="filelist">
            {rows.map((r) => {
              const o = r.outcome;
              const state = r.missing ? "failed" : !o ? "waiting" : o.ok ? "done" : "failed";
              const cut =
                o && o.ok && o.in_bytes > 0
                  ? Math.round((1 - o.out_bytes / o.in_bytes) * 1000) / 10
                  : null;
              return (
                <div key={r.path} className={`row is-${state}`}>
                  <span className="row__thumb" />
                  <span className="row__name" title={r.path}>
                    {r.name}
                  </span>
                  <span className="row__from">{fmtSize(o?.in_bytes ?? r.bytes)}</span>
                  <span className="row__to">
                    {r.missing || (o && !o.ok) ? t("run.failed") : o ? fmtSize(o.out_bytes) : "—"}
                  </span>
                  <span className="pill">
                    {r.missing
                      ? "×"
                      : !o
                        ? t("run.waiting")
                        : o.ok
                          ? cut !== null && cut > 0
                            ? `−${cut.toFixed(1)}%`
                            : t("run.grew")
                          : "×"}
                  </span>
                  {o?.note && <span className="row__note">{o.note}</span>}
                  {/* 后端错误优先；没跑过时才用前端预检的结果，避免同一条错误显示两遍 */}
                  {(o?.error || r.missing) && (
                    <span className="row__error">
                      {o?.error ? t(o.error.key as never, o.error.vars) : t("err.notFound")}
                    </span>
                  )}
                </div>
              );
            })}
          </div>

          <p className="lede">{t("run.outputTo", { dir: "Baobox_output" })}</p>

          <div className="runbar">
            <button className="go" onClick={run} disabled={notReady || running}>
              {running ? t("run.running") : summary ? t("run.done") : t("run.start")}
            </button>
            {summary && !running && outputDirRef.current && (
              <button
                className="chip"
                onClick={() => revealItemInDir(outputDirRef.current!)}
              >
                {t("run.openOutput")}
              </button>
            )}
            <div className="meter">
              <i className="meter__fill" style={{ width: `${pct}%` }} />
            </div>
          </div>

          {summary && <p className="lede">{summary}</p>}
        </>
      )}
    </>
  );
}

function OptionControl({
  def,
  value,
  onChange,
}: {
  def: OptionDef;
  value: string | number | boolean;
  onChange: (v: string | number | boolean) => void;
}) {
  const { t } = useI18n();
  const label = t(def.label as never);

  if (def.kind === "number")
    return (
      <div className="opt">
        <span className="opt__label">{label}</span>
        <span className="opt__value">{String(value)}</span>
        {def.unit && <span className="opt__unit">{def.unit}</span>}
        <input
          type="range"
          min={def.min}
          max={def.max}
          step={def.step}
          value={Number(value)}
          aria-label={label}
          onChange={(e) => onChange(Number(e.target.value))}
        />
      </div>
    );

  if (def.kind === "choice")
    return (
      <div className="opt">
        <span className="opt__label">{label}</span>
        {def.choices.map((c) => (
          <button
            key={c.value}
            className="chip"
            aria-pressed={value === c.value}
            onClick={() => onChange(c.value)}
          >
            {c.label.startsWith("opt.") ? t(c.label as never) : c.label}
          </button>
        ))}
      </div>
    );

  if (def.kind === "toggle")
    return (
      <div className="opt">
        <span className="opt__label">{label}</span>
        <button className="chip" aria-pressed={Boolean(value)} onClick={() => onChange(!value)}>
          {value ? "ON" : "OFF"}
        </button>
      </div>
    );

  return (
    <div className="opt">
      <span className="opt__label">{label}</span>
      <input
        type={def.id === "password" ? "password" : "text"}
        value={String(value)}
        placeholder={def.placeholder ? t(def.placeholder as never) : undefined}
        aria-label={label}
        onChange={(e) => onChange(e.target.value)}
      />
    </div>
  );
}
