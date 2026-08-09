import { useCallback, useEffect, useState } from "react";
import { storageGet, storageSet } from "./storage";
import { findTool } from "./tools/registry";

/**
 * 「最近用过」需要跨会话保留。
 *
 * 原先只是个 useState，关掉软件就没了——而「最近」的全部意义就在于
 * 下次打开还在那儿。工具有 22 个，常用的往往只有两三个。
 */
const KEY = "baobox.recent";
const MAX = 4;

export function useRecent() {
  const [recent, setRecent] = useState<string[]>(() => {
    try {
      const raw = JSON.parse(storageGet(KEY) ?? "[]");
      // 工具可能被改名或移除，存下来的 id 得再验一遍
      return Array.isArray(raw)
        ? raw
            .filter((id): id is string =>
              typeof id === "string" && findTool(id)?.status === "ready",
            )
            .slice(0, MAX)
        : [];
    } catch {
      return [];
    }
  });

  useEffect(() => {
    storageSet(KEY, JSON.stringify(recent));
  }, [recent]);

  const push = useCallback((id: string) => {
    setRecent((prev) => [id, ...prev.filter((x) => x !== id)].slice(0, MAX));
  }, []);

  return { recent, push };
}
