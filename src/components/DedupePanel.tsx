import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useI18n } from "../i18n";
import { asAppErr } from "../errText";
import { ToolHead } from "./ToolHead";
import { useFocusTrap } from "../useFocusTrap";
import { fmtSize } from "../useSaved";

/**
 * 重复文件查找。
 *
 * 流程是「扫描 → 分组 → 勾选 → 删除」，和其他工具的
 * 「拖入 → 配置 → 执行」不一样，所以单独一个面板。
 *
 * 删除确认是这里最要紧的部分：列清单、报总量、默认焦点在取消，
 * 并且明确告知进的是回收站——误删能原地还原。
 */

interface DupFile {
  path: string;
  name: string;
  size: number;
  modified: number;
  keep: boolean;
  /** 归某个程序/环境管辖，删了会把它弄坏 */
  managed: string | null;
}
interface DupGroup {
  size: number;
  files: DupFile[];
  reclaimable: number;
}
interface DupReport {
  groups: DupGroup[];
  scanned: number;
  total_reclaimable: number;
  unreadable: number;
  skipped_cloud: number;
  managed_groups: number;
  cancelled: boolean;
}

const PHASE_KEY: Record<string, string> = {
  walk: "dedupe.phaseWalk",
  quick: "dedupe.phaseQuick",
  full: "dedupe.phaseFull",
  done: "dedupe.phaseDone",
};

export function DedupePanel({
  onSaved,
  onDone,
  onHistory,
}: {
  onSaved: (bytes: number) => void;
  /** 扫描结束时回调本次耗时。全盘扫描是整个软件里最长的操作，
      也最可能被切走去干别的，提示音在这里最有意义。 */
  onDone?: (elapsedMs: number) => void;
  onHistory?: (e: { toolId: string; summary: string; outPath: string | null }) => void;
}) {
  const { t } = useI18n();
  const [roots, setRoots] = useState<string[]>([]);
  const [report, setReport] = useState<DupReport | null>(null);
  const [scanning, setScanning] = useState(false);
  const [phase, setPhase] = useState<{ phase: string; done: number; total: number } | null>(null);
  /** 勾选待删的路径 */
  const [doomed, setDoomed] = useState<Set<string>>(new Set());
  const [confirming, setConfirming] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    const un = getCurrentWebview().listen<{ phase: string; done: number; total: number }>(
      "baobox://scan",
      ({ payload }) => setPhase(payload),
    );
    return () => {
      un.then((f) => f());
    };
  }, []);

  const pickFolder = async () => {
    const sel = await open({ directory: true, multiple: true });
    if (Array.isArray(sel)) setRoots((r) => [...new Set([...r, ...sel])]);
    else if (typeof sel === "string") setRoots((r) => [...new Set([...r, sel])]);
  };

  const scan = async () => {
    if (roots.length === 0 || scanning) return;
    const startedAt = performance.now();
    setScanning(true);
    setReport(null);
    setDoomed(new Set());
    setErr(null);
    try {
      const r = await invoke<DupReport>("find_duplicates", { roots });
      setReport(r);
      // 每组默认保留一份，其余预勾选——但删除仍需用户再确认一次
      const pre = new Set<string>();
      r.groups.forEach((g) => g.files.forEach((f) => !f.keep && pre.add(f.path)));
      setDoomed(pre);
    } catch (e) {
      const ae = asAppErr(e);
      setErr(t(ae.key as never, ae.vars));
    } finally {
      setScanning(false);
      setPhase(null);
      onDone?.(performance.now() - startedAt);
    }
  };

  const toggle = useCallback((path: string) => {
    setDoomed((prev) => {
      const next = new Set(prev);
      next.has(path) ? next.delete(path) : next.add(path);
      return next;
    });
  }, []);

  const doomedList = useMemo(() => {
    if (!report) return [];
    return report.groups.flatMap((g) => g.files.filter((f) => doomed.has(f.path)));
  }, [report, doomed]);

  const doomedBytes = useMemo(
    () => doomedList.reduce((n, f) => n + f.size, 0),
    [doomedList],
  );

  /** 每组至少留一份，一组全被勾掉就说明用户要把这份内容彻底删光 */
  const wipedGroups = useMemo(() => {
    if (!report) return 0;
    return report.groups.filter((g) => g.files.every((f) => doomed.has(f.path))).length;
  }, [report, doomed]);

  const doDelete = async () => {
    const paths = doomedList.map((f) => f.path);
    setErr(null);
    let results: { path: string; ok: boolean }[];
    try {
      results = await invoke<{ path: string; ok: boolean }[]>("delete_to_trash", { paths });
    } catch (e) {
      const ae = asAppErr(e);
      setErr(t(ae.key as never, ae.vars));
      setConfirming(false);
      return;
    }
    const okPaths = new Set(results.filter((r) => r.ok).map((r) => r.path));
    const freed = doomedList.filter((f) => okPaths.has(f.path)).reduce((n, f) => n + f.size, 0);
    onSaved(freed);
    if (okPaths.size > 0) {
      onHistory?.({
        toolId: "file.dedupe",
        summary: `${t("history.items", { n: okPaths.size })} · ${fmtSize(freed)}`,
        outPath: null,
      });
    }
    setConfirming(false);
    // 删掉的从结果里移走，剩下的继续可操作
    setReport((prev) =>
      prev
        ? {
            ...prev,
            groups: prev.groups
              .map((g) => ({ ...g, files: g.files.filter((f) => !okPaths.has(f.path)) }))
              .filter((g) => g.files.length > 1),
          }
        : prev,
    );
    setDoomed(new Set());
  };

  const confirmRef = useRef<HTMLDivElement>(null);
  const confirmTitleId = useId();
  useFocusTrap(confirmRef, confirming, () => setConfirming(false));

  return (
    <div className="toolpage">
      <ToolHead id="file.dedupe" />
      <p className="lede">{t("tool.file.dedupe.desc")}</p>

      <button className="addbar" onClick={pickFolder}>
        <span className="addbar__plus">＋</span>
        {roots.length === 0
          ? t("dedupe.pickFolder")
          : t("dedupe.roots", { count: roots.length, list: roots.join(" · ") })}
      </button>

      {err && (
        <div className="notice notice--bad">
          <span className="notice__mark">!</span>
          <span>{err}</span>
        </div>
      )}

      {report === null ? (
        <div className="empty">
          <div className="empty__box">⌕</div>
          <h2 className="empty__title">{t("dedupe.emptyTitle")}</h2>
          <p className="empty__hint">{t("dedupe.emptyHint")}</p>
        </div>
      ) : (
        <>
          <div className="notice">
            <span className="notice__mark">i</span>
            <span>
              {t("dedupe.summary", {
                scanned: report.scanned,
                groups: report.groups.length,
                size: fmtSize(report.total_reclaimable),
              })}
              {report.skipped_cloud > 0 &&
                ` · ${t("dedupe.skippedCloud", { count: report.skipped_cloud })}`}
            </span>
          </div>

          {report.cancelled && (
            <div className="notice">
              <span className="notice__mark">!</span>
              <span>{t("dedupe.cancelledWarn")}</span>
            </div>
          )}

          {report.managed_groups > 0 && (
            <div className="notice">
              <span className="notice__mark">!</span>
              <span>{t("dedupe.managedWarn", { count: report.managed_groups })}</span>
            </div>
          )}

          <div className="filelist">
            {report.groups.map((g, gi) => (
              <div key={gi} className="dupgroup">
                <div className="dupgroup__head">
                  {t("dedupe.groupHead", {
                    count: g.files.length,
                    each: fmtSize(g.size),
                    save: fmtSize(g.reclaimable),
                  })}
                </div>
                {g.files.map((f) => (
                  <label
                    key={f.path}
                    className={`dupfile${f.managed ? " is-managed" : ""}`}
                  >
                    <input
                      type="checkbox"
                      checked={doomed.has(f.path)}
                      onChange={() => toggle(f.path)}
                    />
                    <span className="dupfile__name" title={f.path}>
                      {f.name}
                      {f.managed && (
                        <span className="dupfile__tag" title={t("dedupe.managedWhy")}>
                          {f.managed}
                        </span>
                      )}
                    </span>
                    <span className="dupfile__path" title={f.path}>
                      {f.path}
                    </span>
                    <button
                      className="chip"
                      onClick={(e) => {
                        e.preventDefault();
                        revealItemInDir(f.path);
                      }}
                    >
                      {t("result.showInFolder")}
                    </button>
                  </label>
                ))}
              </div>
            ))}
          </div>
        </>
      )}

      <div className="runbar">
        <button className="go" onClick={scan} disabled={roots.length === 0 || scanning}>
          {scanning
            ? phase
              ? `${t(PHASE_KEY[phase.phase] as never)} ${phase.done}${phase.total ? "/" + phase.total : ""}`
              : t("dedupe.scanning")
            : t("dedupe.scan")}
        </button>
        {/* 整盘扫描实测要 8.9 分钟，没有退出口是不可接受的 */}
        {scanning && (
          <button className="chip" onClick={() => invoke("cancel_scan")}>
            {t("run.cancel")}
          </button>
        )}
        {doomedList.length > 0 && !scanning && (
          <button className="chip is-danger" onClick={() => setConfirming(true)}>
            {t("dedupe.deleteBtn", { count: doomedList.length, size: fmtSize(doomedBytes) })}
          </button>
        )}
      </div>

      {confirming && (
        <div className="confirm" onMouseDown={() => setConfirming(false)}>
          <div
            className="confirm__box"
            role="dialog"
            aria-modal="true"
            aria-labelledby={confirmTitleId}
            tabIndex={-1}
            ref={confirmRef}
            onMouseDown={(e) => e.stopPropagation()}
          >
            <h2 className="confirm__title" id={confirmTitleId}>{t("dedupe.confirmTitle")}</h2>
            <p className="confirm__lead">
              {t("dedupe.confirmLead", {
                count: doomedList.length,
                size: fmtSize(doomedBytes),
              })}
            </p>
            {wipedGroups > 0 && (
              <p className="confirm__warn">
                {t("dedupe.confirmWipe", { count: wipedGroups })}
              </p>
            )}
            {/* 列全部待删项，不截断——最后一步用户必须能核对到底要删哪些。
                容器本身可滚动，多也翻得完（安全红线 4）。 */}
            <div className="confirm__list">
              {doomedList.map((f) => (
                <div key={f.path} className="confirm__row" title={f.path}>
                  {f.path}
                </div>
              ))}
            </div>
            <p className="confirm__safe">{t("dedupe.recycleNote")}</p>
            <div className="confirm__actions">
              {/* 默认焦点在取消：这类操作宁可多点一次，也不能手滑 */}
              <button className="go" data-autofocus onClick={() => setConfirming(false)}>
                {t("run.cancel")}
              </button>
              <button className="chip is-danger" onClick={doDelete}>
                {t("dedupe.confirmGo")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
