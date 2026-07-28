import { getCurrentWindow } from "@tauri-apps/api/window";
import { useI18n, type Locale } from "../i18n";
import { TOOLS } from "../tools/registry";

/**
 * 自定义标题栏。
 *
 * 用 Windows 样式的 ─ ▢ ✕ 而不是 macOS 的三个彩色圆点 —— Baobox 是
 * Windows 独占应用，用错窗口控件是懂行的人一眼能看出的硬伤。
 */
export function TitleBar({ onSearch }: { onSearch: () => void }) {
  const { t, locale, setLocale } = useI18n();
  const win = getCurrentWindow();

  const toggleLocale = () => {
    const next: Locale = locale === "zh-CN" ? "en-US" : "zh-CN";
    setLocale(next);
  };

  return (
    <header className="titlebar">
      <span className="titlebar__brand">
        <span className="titlebar__mark">B</span>
        {t("app.name")}
      </span>

      {/* 这块空白负责拖动窗口 */}
      <div className="titlebar__spacer" data-tauri-drag-region />

      <button className="titlebar__search" onClick={onSearch}>
        {t("app.search", { count: TOOLS.length })}
        <span className="titlebar__kbd">Ctrl K</span>
      </button>

      <button className="titlebar__lang" onClick={toggleLocale}>
        {t("lang.switch")}
      </button>

      <div className="wincontrols">
        <button title={t("app.minimize")} onClick={() => win.minimize()}>
          ─
        </button>
        <button title={t("app.maximize")} onClick={() => win.toggleMaximize()}>
          ▢
        </button>
        <button className="is-close" title={t("app.close")} onClick={() => win.close()}>
          ✕
        </button>
      </div>
    </header>
  );
}
