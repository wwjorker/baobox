import { useCallback, useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useI18n } from "./i18n";
import { TitleBar } from "./components/TitleBar";
import { Sidebar } from "./components/Sidebar";
import { CommandPalette } from "./components/CommandPalette";
import { Crumb } from "./components/Crumb";
import { Dialog } from "./components/Dialog";
import { ToolRunner } from "./components/ToolRunner";
import { DedupePanel } from "./components/DedupePanel";
import { RenamePanel } from "./components/RenamePanel";
import { ScreenOcrPanel } from "./components/ScreenOcrPanel";
import { RedactPanel } from "./components/RedactPanel";
import { findTool, toolsOf, type Pillar } from "./tools/registry";
import { fmtSize, useSaved } from "./useSaved";
import { useRecent } from "./useRecent";
import { useChime } from "./useChime";
import "./styles/app.css";

const IMAGE_EXT = ["jpg", "jpeg", "png", "webp", "bmp", "tif", "tiff"];

/**
 * 拖进来的一堆文件属于哪根支柱。
 *
 * 只认类型，不替用户猜动作 —— 拖一份 PDF 进来，想合并、想压缩还是想转图片
 * 只有他自己知道，直接跳进「合并」是自作聪明。省掉找菜单这一步就够了，
 * 剩下的一步让他点。
 */
/** 一行太长，拆成几段贴出来才读得下去 */
const CSP = __APP_CSP__.split("; ").join(";\n");

function guessPillar(paths: string[]): Pillar | null {
  const exts = paths.map((p) => p.split(".").pop()?.toLowerCase() ?? "");
  if (exts.some((e) => e === "pdf")) return "pdf";
  if (exts.some((e) => IMAGE_EXT.includes(e))) return "image";
  return null;
}

export default function App() {
  const { t } = useI18n();
  const [pillar, setPillar] = useState<Pillar>("image");
  const [toolId, setToolId] = useState<string | null>(null);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [offlineOpen, setOfflineOpen] = useState(false);
  const [savedOpen, setSavedOpen] = useState(false);
  const [dropHint, setDropHint] = useState<string | null>(null);
  const [over, setOver] = useState(false);
  /** 首页收下、还没送进任何工具的文件 */
  const [pending, setPending] = useState<string[] | null>(null);
  const { saved, addSaved, resetSaved } = useSaved();
  const { recent, push: pushRecent } = useRecent();
  const { chimeEnabled, toggleChime, chime } = useChime();
  /** 热键触发的计数器，变化即表示要立刻开抓 */
  const [hotkeyTick, setHotkeyTick] = useState(0);

  const tool = toolId ? findTool(toolId) : undefined;

  const openTool = useCallback(
    (id: string) => {
      const def = findTool(id);
      if (!def) return;
      setPillar(def.pillar);
      setToolId(id);
      pushRecent(id);
      // pending 不在这里清 —— 退回来换个工具再试一次是常见动作，
      // 让文件留着比逼用户重拖一遍好。ToolRunner 按路径去重，重复注入无害。
    },
    [pushRecent],
  );

  // 全局热键：把窗口拉起来、跳到截图取字、立刻抓屏
  useEffect(() => {
    const un = getCurrentWebview().listen("baobox://hotkey-screen-ocr", () => {
      setPillar("ocr");
      setToolId("ocr.screen");
      setHotkeyTick((n) => n + 1);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  // 首页拖拽识别：拖 PDF 跳 PDF 工具，拖图片跳图片工具。
  // 网页版做不到这个——它拿不到真实文件类型也没法预先分流。
  useEffect(() => {
    if (tool) {
      setOver(false); // 拖到一半点进工具页，高亮不能留在那儿
      return; // 工具页里由各自的面板处理拖放
    }
    const un = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "over") {
        setOver(true);
        return;
      }
      setOver(false);
      if (e.payload.type !== "drop") return;
      const guess = guessPillar(e.payload.paths);
      if (guess) {
        setDropHint(null);
        setPillar(guess);
        setPending(e.payload.paths);
      } else {
        const ext = e.payload.paths[0]?.split(".").pop()?.toUpperCase() ?? "?";
        setDropHint(t("run.homeDropUnknown", { ext }));
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, [tool, openTool, t]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((v) => !v);
        return;
      }
      if (e.key !== "Escape") return;
      // Esc 层层后退：先关弹层，再退回支柱页
      if (paletteOpen) setPaletteOpen(false);
      else if (offlineOpen) setOfflineOpen(false);
      else if (savedOpen) setSavedOpen(false);
      else if (toolId) setToolId(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [paletteOpen, offlineOpen, savedOpen, toolId]);

  const tools = toolsOf(pillar);

  return (
    <div className="shell">
      <TitleBar
        onSearch={() => setPaletteOpen(true)}
        currentTool={tool ? t(`tool.${tool.id}.name` as never) : null}
      />

      <div className="body">
        <Sidebar
          active={pillar}
          recent={recent}
          onPillar={(p) => {
            setPillar(p);
            setToolId(null);
            // 换支柱等于换了件事做，之前接住的文件和上一条提示都不该跟过去
            setPending(null);
            setDropHint(null);
          }}
          onTool={openTool}
        />

        <main className={`main${over && !tool ? " is-dragover" : ""}`}>
          {/* 面包屑对全部工具页统一渲染，各面板不再自己写一份 */}
          {tool && (
            <Crumb
              pillar={tool.pillar}
              name={t(`tool.${tool.id}.name` as never)}
              onBack={() => setToolId(null)}
            />
          )}

          {tool?.id === "image.redact" ? (
            <RedactPanel />
          ) : tool?.id === "ocr.screen" ? (
            <ScreenOcrPanel autoStart={hotkeyTick} />
          ) : tool?.id === "file.rename" ? (
            <RenamePanel />
          ) : tool?.id === "file.dedupe" ? (
            // 「扫描 → 分组 → 勾选 → 删除」和通用的「拖入 → 配置 → 执行」
            // 是两种流程，硬套一个框架只会两边都别扭
            <DedupePanel onSaved={addSaved} onDone={chime} />
          ) : tool ? (
            <ToolRunner
              key={tool.id}
              tool={tool}
              initialPaths={pending}
              onSaved={addSaved}
              onDone={(ms) => {
                chime(ms);
                // 已经跑过了，退回支柱页时不该还挂着「收下了 3 个文件」
                setPending(null);
              }}
            />
          ) : (
            <>
              <h1 className="h1">{t(`pillar.${pillar}` as never)}</h1>
              {/* 每根支柱写自己的介绍。早先这里复用了 App 的标语，
                  四个支柱下面都是同一句「本地文件工作台」，等于没说。 */}
              <p className="lede">{t(`pillar.${pillar}Desc` as never)}</p>

              {/* 拖进来的文件先在这儿等着。只认出了类型、没替用户定动作，
                  所以下一步还是他点，但不用再拖一次。 */}
              {pending && (
                <div className="caught">
                  <span className="caught__mark">✓</span>
                  <span className="caught__text">
                    {t("run.dropped", { count: pending.length })}
                  </span>
                  <button className="caught__clear" onClick={() => setPending(null)}>
                    {t("run.droppedClear")}
                  </button>
                </div>
              )}

              <div className="grid">
                {tools.map((tl) => (
                  <button key={tl.id} className="card" onClick={() => openTool(tl.id)}>
                    <span className="card__name">
                      {t(`tool.${tl.id}.name` as never)}
                      {tl.highlight && (
                        <span className="badge is-highlight">{t("status.highlight")}</span>
                      )}
                      {tl.status !== "ready" && (
                        <span className="badge">{t(`status.${tl.status}` as never)}</span>
                      )}
                    </span>
                    <span className="card__desc">{t(`tool.${tl.id}.desc` as never)}</span>
                  </button>
                ))}
              </div>

              {/* 首页原本是一大片空白。而「拖进来就懂」本来就是设计时
                  列出的差异化点之一，这里正是它该在的位置。 */}
              <div className={`homedrop${over ? " is-over" : ""}`}>
                <span className="homedrop__mark">↓</span>
                <span className="homedrop__title">{t("run.homeDropTitle")}</span>
                <span className="homedrop__hint">{dropHint ?? t("run.homeDropHint")}</span>
              </div>
            </>
          )}
        </main>
      </div>

      <footer className="statusbar">
        {/* 「全程离线」是整个产品最核心的承诺，不该是全屏最灰的一行字。
            而且一句永不变的标语看几次就成了墙纸——做成可点开验证的。 */}
        <button
          className="offlineplate"
          title={t("app.offlineMore")}
          onClick={() => setOfflineOpen(true)}
        >
          <span className="statusbar__dot" />
          {t("app.offline")}
          <span className="offlineplate__more">?</span>
        </button>

        <button className="statusbar__toggle" onClick={toggleChime}>
          {chimeEnabled ? t("app.soundOn") : t("app.soundOff")}
        </button>

        <span className="statusbar__version">v{__APP_VERSION__}</span>

        {/* 数字得说清楚是怎么来的，否则它只是个好看的牌子。
            点开能看到口径，也能归零。 */}
        <button className="savedplate" onClick={() => setSavedOpen(true)}>
          <span className="savedplate__label">{t("app.saved")}</span>
          <span className="savedplate__value">{fmtSize(saved)}</span>
        </button>
      </footer>

      {offlineOpen && (
        <Dialog title={t("app.offline")} onClose={() => setOfflineOpen(false)}>
          <p className="confirm__lead">{t("app.offlineWhy")}</p>
          <pre className="evidence">{CSP}</pre>
          <p className="confirm__safe">{t("app.offlineCsp")}</p>
        </Dialog>
      )}

      {savedOpen && (
        <Dialog
          title={t("app.saved")}
          onClose={() => setSavedOpen(false)}
          aside={
            <button
              className="chip"
              onClick={() => {
                resetSaved();
                setSavedOpen(false);
              }}
            >
              {t("app.savedReset")}
            </button>
          }
        >
          <p className="confirm__lead">
            {t("app.savedWhat", { total: fmtSize(saved) })}
          </p>
          <p className="confirm__safe">{t("app.savedNotCounted")}</p>
          <p className="confirm__safe">{t("app.savedStale")}</p>
        </Dialog>
      )}

      <CommandPalette
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        onPick={openTool}
      />
    </div>
  );
}
