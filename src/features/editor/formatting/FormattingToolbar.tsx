export type FormatCommand =
  | "heading1"
  | "heading2"
  | "bold"
  | "italic"
  | "strikethrough"
  | "link"
  | "bulletList"
  | "orderedList"
  | "taskList"
  | "quote"
  | "inlineCode"
  | "codeBlock"
  | "table";

type FormattingToolbarProps = Readonly<{
  disabled: boolean;
  onCommand(command: FormatCommand): void;
}>;

const controls: readonly Readonly<{
  command: FormatCommand;
  label: string;
  text: string;
  shortcut?: string;
}>[] = [
  { command: "heading1", label: "Heading 1", text: "H1" },
  { command: "heading2", label: "Heading 2", text: "H2" },
  { command: "bold", label: "Bold", text: "B", shortcut: "Ctrl+B" },
  { command: "italic", label: "Italic", text: "I", shortcut: "Ctrl+I" },
  { command: "strikethrough", label: "Strikethrough", text: "S" },
  { command: "link", label: "Link", text: "Link", shortcut: "Ctrl+K" },
  { command: "bulletList", label: "Bulleted list", text: "• List" },
  { command: "orderedList", label: "Numbered list", text: "1. List" },
  { command: "taskList", label: "Task list", text: "☐ List" },
  { command: "quote", label: "Block quote", text: "Quote" },
  { command: "inlineCode", label: "Inline code", text: "Code" },
  { command: "codeBlock", label: "Code block", text: "Block" },
  { command: "table", label: "Insert table", text: "Table" },
];

export function FormattingToolbar({ disabled, onCommand }: FormattingToolbarProps) {
  return (
    <div className="formatting-toolbar" role="toolbar" aria-label="Markdown formatting">
      {controls.map((control) => (
        <button
          key={control.command}
          type="button"
          disabled={disabled}
          aria-label={control.label}
          title={control.shortcut ? `${control.label} (${control.shortcut})` : control.label}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => onCommand(control.command)}
        >
          {control.text}
        </button>
      ))}
    </div>
  );
}
