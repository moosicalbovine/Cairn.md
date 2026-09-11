import { render, waitFor } from "@testing-library/react";
import type { Editor } from "@milkdown/kit/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { MarkdownSession } from "../session/MarkdownSession";
import { VisualSession } from "../visual/VisualSession";
import { VisualDocumentEditor } from "./VisualDocumentEditor";

const editorMocks = vi.hoisted(() => ({
  create: vi.fn(async (parent: HTMLElement) => {
    parent.dataset.editorMounted = "true";
    return {
      editor: {} as Editor,
      destroy: vi.fn(async () => undefined),
    };
  }),
}));

vi.mock("../visual/createVisualSegmentEditor", () => ({
  createVisualSegmentEditor: editorMocks.create,
}));

type ObserverCallback = IntersectionObserverCallback;

class FakeIntersectionObserver implements IntersectionObserver {
  static readonly instances: FakeIntersectionObserver[] = [];

  readonly root = null;
  readonly rootMargin = "800px 0px";
  readonly scrollMargin = "0px";
  readonly thresholds = [0];
  readonly callback: ObserverCallback;
  readonly observed = new Set<Element>();

  constructor(callback: ObserverCallback) {
    this.callback = callback;
    FakeIntersectionObserver.instances.push(this);
  }

  disconnect(): void {
    this.observed.clear();
  }

  observe(target: Element): void {
    this.observed.add(target);
  }

  takeRecords(): IntersectionObserverEntry[] {
    return [];
  }

  unobserve(target: Element): void {
    this.observed.delete(target);
  }

  reveal(): void {
    const target = this.observed.values().next().value as Element | undefined;
    if (!target) return;
    this.callback(
      [{ isIntersecting: true, target } as IntersectionObserverEntry],
      this,
    );
  }
}

function largeDocument(): string {
  return Array.from({ length: 36 }, (_, index) =>
    [
      `## Section ${index + 1}`,
      "",
      `Paragraph ${index + 1}. `.repeat(420),
    ].join("\n"),
  ).join("\n\n");
}

describe("VisualDocumentEditor large-document mounting", () => {
  beforeEach(() => {
    editorMocks.create.mockClear();
    FakeIntersectionObserver.instances.length = 0;
    vi.stubGlobal("IntersectionObserver", FakeIntersectionObserver);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("mounts the first visual section eagerly and defers later sections", async () => {
    const visual = new VisualSession(MarkdownSession.fromSource(largeDocument()));

    render(<VisualDocumentEditor visual={visual} onRequestSource={vi.fn()} />);

    await waitFor(() => expect(editorMocks.create).toHaveBeenCalledTimes(1));
    expect(FakeIntersectionObserver.instances.length).toBeGreaterThan(0);

    FakeIntersectionObserver.instances[0]?.reveal();
    await waitFor(() => expect(editorMocks.create).toHaveBeenCalledTimes(2));

    visual.dispose();
  });
});
