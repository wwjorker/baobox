import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../i18n";

/**
 * 批量重命名
 *
 * 重命名一旦执行就散落在文件系统各处，没有预览等于闭眼操作。
 * 所以规则每改一次就重算预览，冲突和非法字符当场标红；
 * 执行后再留一份撤销日志——预览也会看漏，那是最后一道防线。
 */

type Rule =
  | { kind: "replace"; find: string; replace: string }
  | { kind: "regex"; find: string; replace: string }
  | { kind: "prefix"; text: string }
  | { kind: "suffix"; text: string }
  | { kind: "number"; start: number; digits: number; prefix: boolean }
  | { kind: "case"; mode: string };

interface Preview {
  path: string;
  old_name: string;
  new_name: string;
  conflict: boolean;
  invalid: boolean;
  unchanged: boolean;
}

const NEW_RULE: Record<string, Rule> = {
  replace: { kind: "replace", find: "", replace: "" },
  regex: { kind: "regex", find: "", replace: "" },
  prefix: { kind: "prefix", text: "" },
  suffix: { kind: "suffix", text: "" },
  number: { kind: "number", start: 1, digits: 2, prefix: false },
  case: { kind: "case", mode: "lower" },
};

export function RenamePanel() {
  const { t } = useI18n();
  const [paths, setPaths] = useState<string[]>([]);
  const [rules, setRules] = useState<Rule[]>([]);
  const [preview, setPreview] = useState<Preview[]>([]);
  const [undoLog, setUndoLog] = useState<string | null>(null);
  const [result, setResult] = useState<{ done: number; skipped: number; failed: number } | null>(
    null,
  );

  const addPaths = useCallback((incoming: string[]) => {
    setPaths((prev) => [...new Set([...prev, ...incoming])]);
    setResult(null);
    setUndoLog(null);
  }, []);

  useEffect(() => {
    const un = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "drop") addPaths(e.payload.paths);
    });
    return () => {
      un.then((f) => f());
    };
  }, [addPaths]);

  // 规则或文件一变就重算预览。这是这个工具的核心——
  // 用户必须在按下执行之前就看到每个文件会变成什么样。
  useEffect(() => {
    if (paths.length === 0) {
      setPreview([]);
      return;
    }
    let alive = true;
    invoke<Preview[]>("rename_preview", { paths, rules }).then((p) => {
      if (alive) setPreview(p);
    });
    return () => {
      alive = false;
    };
  }, [paths, rules]);

  const pick = async () => {
    const sel = await open({ multiple: true });
    if (Array.isArray(sel)) addPaths(sel);
    else if (typeof sel === "string") addPaths([sel]);
  };

  const apply = async () => {
    const r = await invoke<{ done: number; skipped: number; failed: number; undo_log: string }>(
      "rename_apply",
      { paths, rules },
    );
    setResult(r);
    setUndoLog(r.undo_log);
    // 名字变了，路径也就变了，清空重来避免拿旧路径继续操作
    setPaths([]);
    setPreview([]);
  };

  const undo = async () => {
    if (!undoLog) return;
    const n = await invoke<number>("rename_undo", { logPath: undoLog });
    setUndoLog(null);
    setResult(null);
    alert(t("rename.undone", { count: n }));
  };

  const applicable = preview.filter((p) => !p.conflict && !p.invalid && !p.unchanged).length;
  const problems = preview.filter((p) => p.conflict || p.invalid).length;

  return (
    <>
      <div className="crumb">
        {t("pillar.file")} <span>›</span> <b>{t("tool.file.rename.name")}</b>
      </div>
      <h1 className="h1">{t("tool.file.rename.name")}</h1>
      <p className="lede">{t("tool.file.rename.desc")}</p>

      <button className="addbar" onClick={pick}>
        <span className="addbar__plus">＋</span>
        {paths.length === 0
          ? t("rename.pick")
          : t("rename.picked", { count: paths.length })}
      </button>

      {/* ---- 规则链 ---- */}
      <div className="rules">
        {rules.map((r, i) => (
          <div key={i} className="rule">
            <span className="rule__no">{i + 1}</span>
            <span className="rule__kind">{t(`rename.rule.${r.kind}` as never)}</span>
            {(r.kind === "replace" || r.kind === "regex") && (
              <>
                <input
                  value={r.find}
                  placeholder={t("rename.find")}
                  onChange={(e) =>
                    setRules((p) => p.map((x, j) => (j === i ? { ...r, find: e.target.value } : x)))
                  }
                />
                <span className="rule__arrow">→</span>
                <input
                  value={r.replace}
                  placeholder={t("rename.replaceWith")}
                  onChange={(e) =>
                    setRules((p) =>
                      p.map((x, j) => (j === i ? { ...r, replace: e.target.value } : x)),
                    )
                  }
                />
              </>
            )}
            {(r.kind === "prefix" || r.kind === "suffix") && (
              <input
                value={r.text}
                placeholder={t("rename.text")}
                onChange={(e) =>
                  setRules((p) => p.map((x, j) => (j === i ? { ...r, text: e.target.value } : x)))
                }
              />
            )}
            {r.kind === "number" && (
              <>
                <span className="opt__label">{t("rename.start")}</span>
                <input
                  type="number"
                  value={r.start}
                  style={{ width: 70 }}
                  onChange={(e) =>
                    setRules((p) =>
                      p.map((x, j) => (j === i ? { ...r, start: +e.target.value } : x)),
                    )
                  }
                />
                <span className="opt__label">{t("rename.digits")}</span>
                <input
                  type="number"
                  value={r.digits}
                  style={{ width: 60 }}
                  onChange={(e) =>
                    setRules((p) =>
                      p.map((x, j) => (j === i ? { ...r, digits: +e.target.value } : x)),
                    )
                  }
                />
                <button
                  className="chip"
                  aria-pressed={r.prefix}
                  onClick={() =>
                    setRules((p) => p.map((x, j) => (j === i ? { ...r, prefix: !r.prefix } : x)))
                  }
                >
                  {r.prefix ? t("rename.atFront") : t("rename.atBack")}
                </button>
              </>
            )}
            {r.kind === "case" &&
              ["lower", "upper", "title"].map((m) => (
                <button
                  key={m}
                  className="chip"
                  aria-pressed={r.mode === m}
                  onClick={() =>
                    setRules((p) => p.map((x, j) => (j === i ? { ...r, mode: m } : x)))
                  }
                >
                  {t(`rename.case.${m}` as never)}
                </button>
              ))}
            <button
              className="chip rule__del"
              onClick={() => setRules((p) => p.filter((_, j) => j !== i))}
            >
              ✕
            </button>
          </div>
        ))}

        <div className="rule rule--add">
          <span className="opt__label">{t("rename.addRule")}</span>
          {Object.keys(NEW_RULE).map((k) => (
            <button
              key={k}
              className="chip"
              onClick={() => setRules((p) => [...p, { ...NEW_RULE[k] }])}
            >
              {t(`rename.rule.${k}` as never)}
            </button>
          ))}
        </div>
      </div>

      {/* ---- 预览 ---- */}
      {preview.length === 0 ? (
        <div className="empty">
          <div className="empty__box">✎</div>
          <h2 className="empty__title">{t("rename.emptyTitle")}</h2>
          <p className="empty__hint">{t("rename.emptyHint")}</p>
        </div>
      ) : (
        <div className="filelist">
          {preview.map((p) => (
            <div
              key={p.path}
              className={`renamerow${p.conflict || p.invalid ? " is-bad" : ""}${p.unchanged ? " is-same" : ""}`}
            >
              <span className="renamerow__old">{p.old_name}</span>
              <span className="renamerow__arrow">→</span>
              <span className="renamerow__new">{p.new_name}</span>
              {p.invalid && <span className="renamerow__tag">{t("rename.invalid")}</span>}
              {p.conflict && !p.invalid && (
                <span className="renamerow__tag">{t("rename.conflict")}</span>
              )}
            </div>
          ))}
        </div>
      )}

      {problems > 0 && (
        <div className="notice">
          <span className="notice__mark">!</span>
          <span>{t("rename.problemNote", { count: problems })}</span>
        </div>
      )}

      <div className="runbar">
        <button className="go" onClick={apply} disabled={applicable === 0}>
          {t("rename.apply", { count: applicable })}
        </button>
        {undoLog && (
          <button className="chip is-danger" onClick={undo}>
            {t("rename.undo")}
          </button>
        )}
      </div>

      {result && (
        <p className="lede">
          {t("rename.result", {
            done: result.done,
            skipped: result.skipped,
            failed: result.failed,
          })}
        </p>
      )}
    </>
  );
}
