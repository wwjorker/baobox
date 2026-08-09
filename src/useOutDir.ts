import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { storageGet, storageRemove, storageSet } from "./storage";

/**
 * 输出位置。
 *
 * 默认「跟着源文件走」——处理完在原地就能找到，对大多数人最省事。
 * 但「我就要全部放到 D 盘那个文件夹」是完全正当的要求，
 * 之前只能接受我们的安排，没有别的选择。
 *
 * 注意两种模式的覆盖策略不同：我们自建的 Baobox_output 里同名产物直接覆盖
 * （否则跑三遍堆出三份），而用户自己指定的目录里可能本来就有他的东西，
 * 一律加后缀，绝不覆盖。这条在后端 unique_path 里。
 */
const KEY = "baobox.outDir";

export function useOutDir() {
  const [outDir, setOutDir] = useState<string | null>(() => storageGet(KEY));

  // 后端保存的是进程内状态，每次启动都要重新告诉它一遍
  useEffect(() => {
    invoke("set_output_dir", { dir: outDir }).catch(() => {
      // 目录被删了或换了盘符——退回默认，别让接下来整批任务都写不出去
      setOutDir(null);
      storageRemove(KEY);
    });
  }, [outDir]);

  const pickOutDir = useCallback(async () => {
    const sel = await open({ directory: true, multiple: false });
    if (typeof sel !== "string") return;
    setOutDir(sel);
    storageSet(KEY, sel);
  }, []);

  const resetOutDir = useCallback(() => {
    setOutDir(null);
    storageRemove(KEY);
  }, []);

  return { outDir, pickOutDir, resetOutDir };
}
