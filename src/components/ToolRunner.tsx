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
  /** 仅文本类工具（OCR 等）会有 */
  text?: string | null;
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
  const [workingOn, setWorkingOn] = useState<string | null>(null);
  const [over, setOver] = useState(false);
  const [values, setValues] = useState<Record<string, string | number | boolean>>(() =>
    Object.fromEntries(tool.options.map((o) => [o.id, o.def])),
  );
  const outputDirRef = useRef<string | null>(null);

  const notReady = tool.status !== "ready";
  const isText = tool.output === "text";
  const accepts = tool.accepts.length ? tool.accepts : ["*"];
  const acceptsLabel = accepts.map((e) => e.toUpperCase()).join(" · ");
  const [langs, setLangs] = useState<string[] | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  // OCR 依赖系统语言包，装没装要提前说清楚，别等识别出空结果让用户纳闷
  useEffect(() => {
    if (tool.pillar !== "ocr") return;
    invoke<string[]>("ocr_languages")
      .then(setLangs)
      .catch(() => setLangs([]));
  }, [tool.pillar]);

  const copy = async (text: string, key: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(key);
      setTimeout(() => setCopied(null), 1600);
    } catch {
      /* 剪贴板被拒时静默失败，不弹错误打断用户 */
    }
  };

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

    // 开工前的「正在处理哪一个」，避免遇到慢文件时界面毫无动静
    const unWorking = await getCurrentWebview().listen<{ name: string; index: number; total: number }>(
      "baobox://working",
      ({ payload }) => setWorkingOn(`${payload.index + 1}/${payload.total} · ${payload.name}`),
    );

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
      unWorking();
      setWorkingOn(null);
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
          <span>
            {tool.notReadyReason
              ? t(tool.notReadyReason as never)
              : t("status.plannedHint")}
          </span>
        </div>
      )}

      {tool.pillar === "ocr" && langs !== null && (
        <div className={langs.length === 0 ? "notice" : "lede"}>
          {langs.length === 0 ? (
            <>
              <span className="notice__mark">!</span>
              <span>{t("err.ocrNoLanguage")}</span>
            </>
          ) : (
            t("run.langs", { list: langs.join(" · ") })
          )}
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
                  dynamic={o.kind === "dynamic-choice" ? (langs ?? []) : undefined}
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
                  {isText && o?.ok && (
                    <div className="textout">
                      <pre className="textout__body">
                        {o.text?.trim() || t("run.noText")}
                      </pre>
                      {o.text?.trim() && (
                        <button
                          className="chip textout__copy"
                          onClick={() => copy(o.text!, r.path)}
                        >
                          {copied === r.path ? t("run.copied") : t("run.copy")}
                        </button>
                      )}
                    </div>
                  )}
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
              {running
                ? workingOn
                  ? `${t("run.running")} ${workingOn}`
                  : t("run.running")
                : summary
                  ? t("run.done")
                  : t("run.start")}
            </button>
            {isText && summary && !running && (
              <button
                className="chip"
                onClick={() =>
                  copy(
                    rows
                      .filter((r) => r.outcome?.text?.trim())
                      .map((r) => r.outcome!.text!.trim())
                      .join("\n\n"),
                    "__all__",
                  )
                }
              >
                {copied === "__all__" ? t("run.copied") : t("run.copyAll")}
              </button>
            )}
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
  dynamic,
  onChange,
}: {
  def: OptionDef;
  value: string | number | boolean;
  /** 运行时探测出来的可选项，如系统已装的 OCR 语言 */
  dynamic?: string[];
  onChange: (v: string | number | boolean) => void;
}) {
  const { t } = useI18n();
  const label = t(def.label as never);

  if (def.kind === "dynamic-choice") {
    // 只装了一种语言时选择器毫无意义，直接不显示
    if (!dynamic || dynamic.length < 2) return null;
    return (
      <div className="opt">
        <span className="opt__label">{label}</span>
        <button className="chip" aria-pressed={value === ""} onClick={() => onChange("")}>
          {t("opt.ocrLangAuto")}
        </button>
        {dynamic.map((tag) => (
          <button
            key={tag}
            className="chip"
            aria-pressed={value === tag}
            onClick={() => onChange(tag)}
          >
            {tag}
          </button>
        ))}
      </div>
    );
  }

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
