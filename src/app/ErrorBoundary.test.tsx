import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ErrorBoundary } from "./ErrorBoundary";

function BrokenChild(): never {
  throw new Error("sensitive document text");
}

describe("ErrorBoundary", () => {
  it("shows a safe recovery message without leaking error content", () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);

    render(
      <ErrorBoundary>
        <BrokenChild />
      </ErrorBoundary>,
    );

    expect(screen.getByRole("alert")).toHaveTextContent(
      "NoteMD could not display this view",
    );
    expect(screen.queryByText(/sensitive document text/i)).not.toBeInTheDocument();
  });
});
