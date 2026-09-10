import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { FormattingToolbar } from "./FormattingToolbar";

describe("Markdown formatting toolbar", () => {
  it("exposes every common portable Markdown control by accessible name", () => {
    render(<FormattingToolbar disabled={false} onCommand={vi.fn()} />);

    for (const name of [
      "Heading 1",
      "Heading 2",
      "Bold",
      "Italic",
      "Strikethrough",
      "Link",
      "Bulleted list",
      "Numbered list",
      "Task list",
      "Block quote",
      "Inline code",
      "Code block",
      "Insert table",
    ]) {
      expect(screen.getByRole("button", { name })).toBeEnabled();
    }
    expect(screen.queryByRole("button", { name: /preview|split/i })).not.toBeInTheDocument();
  });

  it("dispatches the selected formatting command", () => {
    const onCommand = vi.fn();
    render(<FormattingToolbar disabled={false} onCommand={onCommand} />);

    fireEvent.click(screen.getByRole("button", { name: "Bold" }));

    expect(onCommand).toHaveBeenCalledWith("bold");
  });
});
