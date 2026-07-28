import { useEffect, useState } from "react";
import { useI18n } from "./i18n";
import { TitleBar } from "./components/TitleBar";
import { Sidebar } from "./components/Sidebar";
import { CommandPalette } from "./components/CommandPalette";
import { ToolRunner } from "./components/ToolRunner";
import { findTool, toolsOf, type Pillar } from "./tools/registry";
import "./styles/app.css";

export default function App() {
  const { t } = useI18n();
  const [pillar, setPillar] = useState<Pillar>("image");
  const [toolId, setToolId] = useState<string | null>(null);
  const [recent, setRecent] = useState<string[]>([]);
  const [paletteOpen, setPaletteOpen] = useState(false);

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
          {tool ? (
            <ToolRunner key={tool.id} tool={tool} />
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
            <span className="savedplate__value">0 MB</span>
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
