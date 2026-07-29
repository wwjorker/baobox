import { useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { TOOLS } from "../tools/registry";

/**
 * Ctrl+K 命令面板。
 *
 * 80 个工具必须有快速入口——层层点击侧栏找工具是这类软件最大的体验瓶颈。
 * 这既是真效率，也是产品气质的一部分。
 */
export function CommandPalette({
  open,
  onClose,
  onPick,
}: {
  open: boolean;
  onClose: () => void;
  onPick: (id: string) => void;
}) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const hits = useMemo(() => {
    const q = query.trim().toLowerCase();
    const scored = TOOLS.map((tool) => ({
      tool,
      name: t(`tool.${tool.id}.name` as never),
    }));
    if (!q) return scored;
    // 名称和工具 id 都参与匹配，这样输 "pdf" 能把整组 PDF 工具捞出来
    return scored.filter(
      ({ tool, name }) =>
        name.toLowerCase().includes(q) || tool.id.toLowerCase().includes(q),
    );
  }, [query, t]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setCursor(0);
      // 等面板挂载后再聚焦，否则拿不到输入框
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  useEffect(() => {
    setCursor(0);
  }, [query]);

  if (!open) return null;

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") return onClose();
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setCursor((c) => Math.min(c + 1, hits.length - 1));
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      setCursor((c) => Math.max(c - 1, 0));
    }
    if (e.key === "Enter" && hits[cursor]) {
      onPick(hits[cursor].tool.id);
      onClose();
    }
  };

  return (
    <div className="palette" onMouseDown={onClose}>
      <div
        className="palette__box"
        onMouseDown={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
      >
        <input
          ref={inputRef}
          className="palette__input"
          value={query}
          placeholder={t("app.search", { count: TOOLS.length })}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onKeyDown}
        />
        <div className="palette__list">
          {hits.length === 0 && (
            <div className="palette__none">{t("app.noMatch", { q: query.trim() })}</div>
          )}
          {hits.map(({ tool, name }, i) => (
            <button
              key={tool.id}
              className={`palette__row${i === cursor ? " is-selected" : ""}`}
              onMouseEnter={() => setCursor(i)}
              onClick={() => {
                onPick(tool.id);
                onClose();
              }}
            >
              {name}
              {/* 还没实现的工具在这里就标出来，别让人点进去才发现 */}
              {tool.status !== "ready" && (
                <span className="palette__tag is-wip">
                  {t(`status.${tool.status}` as never)}
                </span>
              )}
              <span className="palette__tag">{t(`pillar.${tool.pillar}` as never)}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
