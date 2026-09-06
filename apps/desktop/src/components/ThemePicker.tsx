import type { ResolvedTheme, ThemePreference } from "../theme";

export function ThemePicker({
  resolvedTheme,
  onChange,
}: {
  resolvedTheme: ResolvedTheme;
  onChange: (preference: ThemePreference) => void;
}) {
  const nextTheme: ThemePreference = resolvedTheme === "dark" ? "light" : "dark";
  const currentLabel = resolvedTheme === "dark" ? "深色" : "浅色";
  const nextLabel = resolvedTheme === "dark" ? "浅色" : "深色";

  return (
    <button
      className="theme-toggle"
      type="button"
      aria-label={`当前为${currentLabel}主题，切换到${nextLabel}主题`}
      title={`切换到${nextLabel}主题`}
      onClick={() => onChange(nextTheme)}
    >
      <span aria-hidden="true">{resolvedTheme === "dark" ? "☾" : "☀"}</span>
      <span>{currentLabel}</span>
    </button>
  );
}
