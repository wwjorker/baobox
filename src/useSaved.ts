import { useCallback, useEffect, useState } from "react";

/**
 * 累计「已省下多少」。
 *
 * 这不只是个数字——它是最好的传播素材：用户截图发出去，
 * 「已省下 1.24 GB」本身就是广告。所以要跨会话累计，存在本地。
 */
const KEY = "baobox.savedBytes";

export function useSaved() {
  const [saved, setSaved] = useState<number>(() => {
    const raw = localStorage.getItem(KEY);
    const n = raw ? Number(raw) : 0;
    return Number.isFinite(n) && n >= 0 ? n : 0;
  });

  useEffect(() => {
    localStorage.setItem(KEY, String(saved));
  }, [saved]);

  const addSaved = useCallback((bytes: number) => {
    if (bytes > 0) setSaved((n) => n + bytes);
  }, []);

  /** 归零。之前的口径把 OCR 这类根本不省空间的操作也算了进来，
      老账没法追溯修正，只能给一个清掉重来的入口。 */
  const resetSaved = useCallback(() => setSaved(0), []);

  return { saved, addSaved, resetSaved };
}

export function fmtSize(bytes: number): string {
  if (bytes >= 1 << 30) return `${(bytes / (1 << 30)).toFixed(2)} GB`;
  if (bytes >= 1 << 20) return `${(bytes / (1 << 20)).toFixed(1)} MB`;
  if (bytes >= 1 << 10) return `${Math.round(bytes / (1 << 10))} KB`;
  return `${bytes} B`;
}
