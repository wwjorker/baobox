import { useCallback, useEffect, useState } from "react";
import { storageGet, storageSet } from "./storage";

/**
 * 批量任务完成时的提示音。
 *
 * 场景很具体：把两百张图丢进去然后切去干别的，一声轻响告诉你好了。
 * 但压一张图七十毫秒也「叮」一下就是骚扰，所以：
 *   · 默认关闭
 *   · 只有耗时超过阈值的任务才响
 *
 * 用 Web Audio 现场合成，不打包音频文件——省体积，也省掉一个
 * 「这个 mp3 哪来的、授权是什么」的问题。
 */
const KEY = "baobox.chime";
/** 低于这个耗时就不响，否则单文件操作会变成噪音源 */
const MIN_MS = 1500;

export function useChime() {
  const [enabled, setEnabled] = useState(() => storageGet(KEY) === "1");

  useEffect(() => {
    storageSet(KEY, enabled ? "1" : "0");
  }, [enabled]);

  const chime = useCallback(
    (elapsedMs: number) => {
      if (!enabled || elapsedMs < MIN_MS) return;
      try {
        const ctx = new AudioContext();
        const now = ctx.currentTime;
        // 两个短音，第二个高一点——比单音更像「完成」而不是「出错」
        [880, 1174.7].forEach((freq, i) => {
          const osc = ctx.createOscillator();
          const gain = ctx.createGain();
          osc.type = "sine";
          osc.frequency.value = freq;
          const at = now + i * 0.11;
          gain.gain.setValueAtTime(0, at);
          gain.gain.linearRampToValueAtTime(0.14, at + 0.012);
          gain.gain.exponentialRampToValueAtTime(0.0001, at + 0.2);
          osc.connect(gain).connect(ctx.destination);
          osc.start(at);
          osc.stop(at + 0.22);
        });
        setTimeout(() => ctx.close(), 700);
      } catch {
        /* 音频不可用时静默略过，不该因为提示音打断正事 */
      }
    },
    [enabled],
  );

  return { chimeEnabled: enabled, toggleChime: () => setEnabled((v) => !v), chime };
}
