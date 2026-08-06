import { useCallback, useState } from "react";

/**
 * 操作历史 —— 你做过什么、产物在哪，本地记一笔。
 *
 * 红线：**只存本地、永不上传**（和「不联网」一致）；**可清空**；
 * **粉碎、涂黑密文一律不记**——那两个工具的用意就是不留痕，记下来自相矛盾，
 * 所以它们的面板压根不调用 push。
 */
export interface HistEntry {
  id: string;
  /** epoch 毫秒 */
  time: number;
  toolId: string;
  /** 一句人话概述，如「3 张 → 省下 6.2 MB」 */
  summary: string;
  /** 产物所在目录/文件；没有产物（如截图取字复制到剪贴板）就为 null */
  outPath: string | null;
}

const KEY = "baobox.history";
const MAX = 120;

function load(): HistEntry[] {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return [];
    const arr = JSON.parse(raw);
    return Array.isArray(arr) ? arr : [];
  } catch {
    return [];
  }
}

function save(list: HistEntry[]) {
  // 隐私模式/禁用存储时写入会抛错，吞掉即可——历史丢了不是致命的。
  try {
    localStorage.setItem(KEY, JSON.stringify(list));
  } catch {
    /* ignore */
  }
}

export function useHistory() {
  const [history, setHistory] = useState<HistEntry[]>(() => load());

  const push = useCallback((e: Omit<HistEntry, "id" | "time">) => {
    setHistory((prev) => {
      const next: HistEntry[] = [
        { ...e, id: `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`, time: Date.now() },
        ...prev,
      ].slice(0, MAX);
      save(next);
      return next;
    });
  }, []);

  const clear = useCallback(() => {
    setHistory([]);
    try {
      localStorage.removeItem(KEY);
    } catch {
      /* ignore */
    }
  }, []);

  return { history, pushHistory: push, clearHistory: clear };
}
