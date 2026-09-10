import type { Appearance } from "./appearance";

type AppearanceSelectProps = Readonly<{
  value: Appearance;
  onChange(value: Appearance): void;
}>;

export function AppearanceSelect({ value, onChange }: AppearanceSelectProps) {
  return (
    <label className="appearance-select">
      <span className="sr-only">Appearance</span>
      <select value={value} onChange={(event) => onChange(event.target.value as Appearance)}>
        <option value="followWindows">Follow Windows</option>
        <option value="light">Light</option>
        <option value="dark">Dark</option>
      </select>
    </label>
  );
}
