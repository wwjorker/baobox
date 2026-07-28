import { useMemo, useState } from "react";
import { useI18n } from "../i18n";
import type { OptionDef, ToolDef } from "../tools/registry";

/**
 * 统一处理流框架：拖入 → 配置 → 执行 → 结果
 *
 * 全部 80 个工具共用这一个组件。工具之间的差异由 registry 里的声明
 * 描述驱动，所以新增工具不需要再写一遍界面。
 *
 * 三个状态都必须存在，缺一个就是设计稿式的自欺：
 *   · 空状态 —— 首次打开的第一印象
 *   · 处理中 —— 进度、可取消
 *   · 失败态 —— 真实使用一定会有损坏/加密/格式不支持的文件
 */

export interface FileItem {
  id: string;
  name: string;
  bytes: number;
  state: "waiting" | "done" | "failed";
  resultBytes?: number;
  error?: string;
}

function fmtSize(bytes: number): string {
  if (bytes >= 1 << 20) return `${(bytes / (1 << 20)).toFixed(1)} MB`;
  if (bytes >= 1 << 10) return `${Math.round(bytes / (1 << 10))} KB`;
  return `${bytes} B`;
}

export function ToolRunner({ tool }: { tool: ToolDef }) {
  const { t } = useI18n();
  const [files, setFiles] = useState<FileItem[]>([]);
  const [over, setOver] = useState(false);
  const [values, setValues] = useState<Record<string, string | number | boolean>>(() =>
    Object.fromEntries(tool.options.map((o) => [o.id, o.def])),
  );

  const totalBytes = useMemo(() => files.reduce((n, f) => n + f.bytes, 0), [files]);
  const accepts = tool.accepts.length
    ? tool.accepts.map((e) => e.toUpperCase()).join(" · ")
    : "*";

  // 后端命令还没实现。此处诚实呈现，不做假进度条骗人。
  const notReady = tool.status !== "ready";

  const set = (id: string, v: string | number | boolean) =>
    setValues((prev) => ({ ...prev, [id]: v }));

  const addDemoFiles = () => {
    const now = Date.now();
    setFiles(
      Array.from({ length: 6 }, (_, i) => ({
        id: `${now}-${i}`,
        name: `IMG_${2310 + i}.jpg`,
        bytes: Math.round((2 + Math.random() * 4) * (1 << 20)),
        state: "waiting" as const,
      })),
    );
  };

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

      {files.length === 0 ? (
        <div className="empty">
          <div className="empty__box">＋</div>
          <h2 className="empty__title">{t("run.emptyTitle")}</h2>
          <p className="empty__hint">{t("run.emptyHint", { formats: accepts })}</p>
          <button
            className={`dropzone${over ? " is-over" : ""}`}
            style={{ maxWidth: 420 }}
            onDragOver={(e) => {
              e.preventDefault();
              setOver(true);
            }}
            onDragLeave={() => setOver(false)}
            onDrop={(e) => {
              e.preventDefault();
              setOver(false);
              addDemoFiles();
            }}
            onClick={addDemoFiles}
          >
            <span className="dropzone__title">{t("run.dropHere")}</span>
            <span className="dropzone__hint">{t("run.dropHint")}</span>
          </button>
        </div>
      ) : (
        <>
          {/* 有文件后拖拽区收成一条，把空间让给列表 */}
          <button className="addbar" onClick={addDemoFiles}>
            <span className="addbar__plus">＋</span>
            {t("run.addMore", { count: files.length, size: fmtSize(totalBytes) })}
          </button>

          {tool.options.length > 0 && (
            <div className="optbar">
              {tool.options.map((o) => (
                <OptionControl key={o.id} def={o} value={values[o.id]} onChange={(v) => set(o.id, v)} />
              ))}
            </div>
          )}

          <div className="filelist">
            {files.map((f) => (
              <div key={f.id} className={`row is-${f.state}`}>
                <span className="row__thumb" />
                <span className="row__name">{f.name}</span>
                <span className="row__from">{fmtSize(f.bytes)}</span>
                <span className="row__to">
                  {f.state === "failed"
                    ? t("run.failed")
                    : f.resultBytes
                      ? fmtSize(f.resultBytes)
                      : "—"}
                </span>
                <span className="pill">{f.state === "waiting" ? t("run.waiting") : "—"}</span>
                {f.error && <span className="row__error">{f.error}</span>}
              </div>
            ))}
          </div>

          <p className="lede">{t("run.outputTo", { dir: "Baobox_output/" })}</p>

          <div className="runbar">
            <button className="go" disabled={notReady}>
              {t("run.start")}
            </button>
            <div className="meter">
              <i className="meter__fill" style={{ width: "0%" }} />
            </div>
          </div>
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
