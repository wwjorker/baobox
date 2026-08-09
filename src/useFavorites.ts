import { useCallback, useEffect, useState } from "react";
import { storageGet, storageSet } from "./storage";
import { findTool } from "./tools/registry";

/**
 * 收藏（常用置顶）。
 *
 * 工具到了 60 个，平铺一屏找起来费劲。「最近用过」是自动的、会滚走；
 * 收藏是用户主动钉的，钉住的常用工具在支柱页排到最前、也进侧栏，
 * 一点就到。跟 useRecent 一样跨会话存本地。
 *
 * 不设上限——钉几个是用户自己的事，不该替他限制。存下来的 id 每次
 * 都重新验一遍，工具改名或移除后不会留下点不动的死条目。
 */
const KEY = "baobox.favorites";

export function useFavorites() {
  const [favorites, setFavorites] = useState<string[]>(() => {
    try {
      const raw = JSON.parse(storageGet(KEY) ?? "[]");
      return Array.isArray(raw)
        ? raw.filter((id): id is string => typeof id === "string" && !!findTool(id))
        : [];
    } catch {
      return [];
    }
  });

  useEffect(() => {
    storageSet(KEY, JSON.stringify(favorites));
  }, [favorites]);

  const toggle = useCallback((id: string) => {
    setFavorites((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    );
  }, []);

  const isFavorite = useCallback((id: string) => favorites.includes(id), [favorites]);

  return { favorites, toggle, isFavorite };
}
