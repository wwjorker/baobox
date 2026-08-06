import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../i18n";
import { findTool } from "../tools/registry";
import { ToolIcon, pillarOf } from "../tools/icons";
import { fmtSize } from "../useSaved";
import { CountUp } from "../CountUp";
import { Mascot } from "./Mascot";
import { ToolCard } from "./ToolCard";

/** 「试试看」推的几个冷门但好用的工具，帮用户发现没用过的。 */
const DISCOVER = ["image.stitch", "file.zhconv", "file.qr-make", "image.grid", "pdf.nup"];
const TIP_KEYS = ["home.tip1", "home.tip2", "home.tip3", "home.tip4", "home.tip5"] as const;

interface Props {
  favorites: string[];
  recent: string[];
  saved: number;
  /** 正拖着文件在窗口上 */
  over: boolean;
  /** 拖了不认识的类型时的提示 */
  dropHint: string | null;
  onOpenTool: (id: string) => void;
  onOpenSaved: () => void;
  /** 点英雄区选文件（拖不方便时的另一条路），交给 App 认类型并跳转 */
  onFiles: (paths: string[]) => void;
  isFavorite: (id: string) => boolean;
  onToggleFav: (id: string) => void;
}

export function Home({
  favorites,
  recent,
  saved,
  over,
  dropHint,
  onOpenTool,
  onOpenSaved,
  onFiles,
  isFavorite,
  onToggleFav,
}: Props) {
  const { t } = useI18n();

  // 点箱子蹦一下：Mascot 本就支持,只是一直没接上触发
  const [cheer, setCheer] = useState(0);

  const pickFiles = async () => {
    setCheer((c) => c + 1);
    const sel = await open({ multiple: true });
    if (Array.isArray(sel)) onFiles(sel);
    else if (typeof sel === "string") onFiles([sel]);
  };
  const favTools = favorites.map(findTool).filter((x): x is NonNullable<typeof x> => !!x);
  const discover = DISCOVER.map(findTool).filter(
    (x): x is NonNullable<typeof x> => !!x && x.status === "ready",
  );

  // 轮换小贴士
  const [tipI, setTipI] = useState(0);
  useEffect(() => {
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduce) return;
    const h = setInterval(() => setTipI((i) => (i + 1) % TIP_KEYS.length), 5200);
    return () => clearInterval(h);
  }, []);

  const chip = (id: string) => {
    const tl = findTool(id);
    if (!tl) return null;
    return (
      <button key={id} className={`rchip rchip--${pillarOf(id)}`} onClick={() => onOpenTool(id)}>
        <span className="rchip__ico">
          <ToolIcon id={id} />
        </span>
        {t(`tool.${id}.name` as never)}
      </button>
    );
  };

  return (
    <div className="home">
      <div className="home__hero">
        <button
          type="button"
          className={`catch${over ? " is-over" : ""}`}
          onClick={pickFiles}
          title={t("home.pickTitle")}
        >
          <div className="catch__spot" />
          <Mascot open={over} cheerKey={cheer} />
          <div className="catch__text">
            <div className="catch__hi">{t("home.heroTitle")}</div>
            <div className="catch__sub">{dropHint ?? t("home.heroSub")}</div>
          </div>
        </button>
        <button className="plate" onClick={onOpenSaved}>
          <span className="plate__label">{t("app.saved")}</span>
          <span className="plate__val"><CountUp value={saved} format={fmtSize} /></span>
          <span className="plate__sub">{t("home.savedSub")}</span>
        </button>
      </div>

      <div className="grouplabel">{t("fav.common")}</div>
      {favTools.length > 0 ? (
        <div className="grid">
          {favTools.map((tl) => (
            <ToolCard
              key={tl.id}
              tool={tl}
              fav={isFavorite(tl.id)}
              onFav={() => onToggleFav(tl.id)}
              onOpen={() => onOpenTool(tl.id)}
            />
          ))}
        </div>
      ) : (
        <p className="home__favhint">{t("home.noFav")}</p>
      )}

      {recent.length > 0 && (
        <>
          <div className="grouplabel is-quiet">{t("pillar.recent")}</div>
          <div className="recentstrip">{recent.map(chip)}</div>
        </>
      )}

      {discover.length > 0 && (
        <>
          <div className="grouplabel is-quiet">{t("home.discover")}</div>
          <div className="recentstrip">{discover.map((tl) => chip(tl.id))}</div>
        </>
      )}

      <div className="tip">
        <span className="tip__tag">{t("home.tipTag")}</span>
        <span className="tip__text">{t(TIP_KEYS[tipI] as never)}</span>
      </div>
    </div>
  );
}
