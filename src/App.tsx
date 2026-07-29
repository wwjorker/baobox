import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useI18n } from "./i18n";
import { TitleBar } from "./components/TitleBar";
import { Sidebar } from "./components/Sidebar";
import { CommandPalette } from "./components/CommandPalette";
import { ToolRunner } from "./components/ToolRunner";
import { DedupePanel } from "./components/DedupePanel";
import { RenamePanel } from "./components/RenamePanel";
import { ScreenOcrPanel } from "./components/ScreenOcrPanel";
import { RedactPanel } from "./components/RedactPanel";
import { findTool, toolsOf, type Pillar } from "./tools/registry";
import { fmtSize, useSaved } from "./useSaved";
import "./styles/app.css";

export default function App() {
  const { t } = useI18n();
  const [pillar, setPillar] = useState<Pillar>("image");
  const [toolId, setToolId] = useState<string | null>(null);
  const [recent, setRecent] = useState<string[]>([]);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const { saved, addSaved } = useSaved();
  /** 热键触发的计数器，变化即表示要立刻开抓 */
  const [hotkeyTick, setHotkeyTick] = useState(0);

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

  const tool = toolId ? findTool(toolId) : undefined;

  const openTool = (id: string) => {
    const def = findTool(id);
    if (!def) return;
    setPillar(def.pillar);
    setToolId(id);
    setRecent((prev) => [id, ...prev.filter((x) => x !== id)].slice(0, 3));
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((v) => !v);
      }
      if (e.key === "Escape") setPaletteOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="shell">
      <TitleBar onSearch={() => setPaletteOpen(true)} />

      <div className="body">
        <Sidebar
          active={pillar}
          recent={recent}
          onPillar={(p) => {
            setPillar(p);
            setToolId(null);
          }}
          onTool={openTool}
        />

        <main className="main">
          {tool?.id === "image.redact" ? (
            <RedactPanel />
          ) : tool?.id === "ocr.screen" ? (
            <ScreenOcrPanel autoStart={hotkeyTick} />
          ) : tool?.id === "file.rename" ? (
            <RenamePanel />
          ) : tool?.id === "file.dedupe" ? (
            // 「扫描 → 分组 → 勾选 → 删除」和通用的「拖入 → 配置 → 执行」
            // 是两种流程，硬套一个框架只会两边都别扭
            <DedupePanel onSaved={addSaved} />
          ) : tool ? (
            <ToolRunner key={tool.id} tool={tool} onSaved={addSaved} />
          ) : (
            <>
              <div className="crumb">
                <b>{t(`pillar.${pillar}` as never)}</b>
              </div>
              <h1 className="h1">{t(`pillar.${pillar}` as never)}</h1>
              <p className="lede">{t("app.tagline")}</p>

              <div className="grid">
                {toolsOf(pillar).map((tl) => (
                  <button key={tl.id} className="card" onClick={() => openTool(tl.id)}>
                    <span className="card__name">
                      {t(`tool.${tl.id}.name` as never)}
                      {tl.highlight && (
                        <span className="badge is-highlight">{t("status.highlight")}</span>
                      )}
                    </span>
                    <span className="card__desc">{t(`tool.${tl.id}.desc` as never)}</span>
                  </button>
                ))}
              </div>
            </>
          )}
        </main>
      </div>

      <footer className="statusbar">
        <span className="statusbar__dot" />
        <span>{t("app.offline")}</span>
        <span className="statusbar__push">
          <span className="savedplate">
            <span className="savedplate__label">{t("app.saved")}</span>
            <span className="savedplate__value">{fmtSize(saved)}</span>
          </span>
        </span>
      </footer>

      <CommandPalette
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        onPick={openTool}
      />
    </div>
  );
}
