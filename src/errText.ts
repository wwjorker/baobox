/**
 * 把一次 invoke 的拒绝原因收敛成「可翻译的错误」。
 *
 * 后端命令返回 AppResult 时，Err 会以 { key, vars, detail } 的形状 reject
 * 到 JS 这边。之前多处 invoke 只有 try/finally 没有 catch——命令整体失败、
 * 反序列化出错或后端 panic 时，界面只是停转，一句提示都没有。
 * 统一走这里：认得的 key 交给 i18n 翻成人话，认不得的兜底成 err.unknown。
 */
export interface AppErr {
  key: string;
  vars?: Record<string, string>;
}

export function asAppErr(e: unknown): AppErr {
  if (e && typeof e === "object" && "key" in e) {
    const ae = e as AppErr;
    if (typeof ae.key === "string") return { key: ae.key, vars: ae.vars };
  }
  return { key: "err.unknown", vars: { detail: typeof e === "string" ? e : String(e) } };
}
