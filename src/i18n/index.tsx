import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import zhCN from "./zh-CN";
import enUS from "./en-US";

/**
 * 界面国际化。
 *
 * 这一层必须在写第一个功能之前就位（方案风险 15）：项目面向 GitHub，
 * 英文用户是主力，等 80 个功能写完再回头抽文案是灾难级工作量。
 *
 * 没有引入 i18n 库——需求只有「查表 + 占位符替换」两件事，
 * 自己写二十行比拖进来一个几十 KB 的依赖更合算。
 */

export const LOCALES = { "zh-CN": zhCN, "en-US": enUS } as const;
export type Locale = keyof typeof LOCALES;

type Join<K, P> = K extends string
  ? P extends string
    ? P extends ""
      ? K
      : `${K}.${P}`
    : never
  : never;

/** 递归展开成点分路径，让 t() 有自动补全并能在编译期抓到拼错的 key */
type Leaves<T> = T extends string
  ? ""
  : { [K in keyof T & string]: Join<K, Leaves<T[K]>> }[keyof T & string];

export type TKey = Leaves<typeof zhCN>;
export type TVars = Record<string, string | number>;

function lookup(dict: unknown, path: string): string {
  const hit = path
    .split(".")
    .reduce<unknown>((node, seg) => (node as Record<string, unknown>)?.[seg], dict);
  // 缺 key 时返回 key 本身，界面上一眼能看出来，而不是渲染成空白
  return typeof hit === "string" ? hit : path;
}

function interpolate(tpl: string, vars?: TVars): string {
  if (!vars) return tpl;
  return tpl.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in vars ? String(vars[name]) : whole,
  );
}

const STORAGE_KEY = "baobox.locale";

function detectLocale(): Locale {
  const saved = localStorage.getItem(STORAGE_KEY);
  if (saved && saved in LOCALES) return saved as Locale;
  return navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en-US";
}

interface I18nValue {
  locale: Locale;
  setLocale: (l: Locale) => void;
  t: (key: TKey, vars?: TVars) => string;
}

const I18nContext = createContext<I18nValue | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(detectLocale);

  const setLocale = useCallback((l: Locale) => {
    localStorage.setItem(STORAGE_KEY, l);
    setLocaleState(l);
    document.documentElement.lang = l;
  }, []);

  const t = useCallback(
    (key: TKey, vars?: TVars) => interpolate(lookup(LOCALES[locale], key), vars),
    [locale],
  );

  const value = useMemo(() => ({ locale, setLocale, t }), [locale, setLocale, t]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nValue {
  const ctx = useContext(I18nContext);
  if (!ctx) throw new Error("useI18n 必须在 <I18nProvider> 内使用");
  return ctx;
}
