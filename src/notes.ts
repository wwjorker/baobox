import type { TKey, TVars } from "./i18n";

/**
 * 结果行右边那句说明。
 *
 * 后端只给 key 和占位符，成句在这里做——早先后端直接拼中文字符串，
 * 英文界面下整整一列都是中文，i18n 层等于在最后一米漏掉了。
 *
 * 拆成 parts 是因为像「质量 78 · 缩放 62% · 未能达标」这种说明是按情况拼的，
 * 拼好的整句没法翻译，拆开每段各自查表就行。
 */
export interface NotePart {
  key: string;
  vars: Record<string, string>;
}

export interface Note {
  parts: NotePart[];
}

export function noteText(
  note: Note | null | undefined,
  t: (key: TKey, vars?: TVars) => string,
): string | null {
  if (!note?.parts?.length) return null;
  return note.parts.map((p) => t(p.key as TKey, p.vars)).join(" · ");
}
