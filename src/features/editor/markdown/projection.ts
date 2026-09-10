import { unified } from "unified";
import remarkGfm from "remark-gfm";
import remarkParse from "remark-parse";

export type MarkdownProjectionKind = "visual" | "source-backed";

export interface MarkdownProjectionSegment {
  readonly id: string;
  readonly kind: MarkdownProjectionKind;
  readonly from: number;
  readonly to: number;
  readonly source: string;
  readonly nodeType: string;
}

export interface MarkdownProjection {
  readonly revision: number;
  readonly segments: readonly MarkdownProjectionSegment[];
}

interface PositionedNode {
  readonly type: string;
  readonly children?: readonly PositionedNode[];
  readonly position?: {
    readonly start?: { readonly offset?: number };
    readonly end?: { readonly offset?: number };
  };
}

interface SourceRange {
  readonly from: number;
  readonly to: number;
}

const unsupportedNodeTypes = new Set(["html"]);

function nodeRange(node: PositionedNode): SourceRange | undefined {
  const from = node.position?.start?.offset;
  const to = node.position?.end?.offset;
  if (from === undefined || to === undefined) {
    return undefined;
  }
  return { from, to };
}

function hasUnsupportedDescendant(node: PositionedNode): boolean {
  if (unsupportedNodeTypes.has(node.type)) {
    return true;
  }
  return node.children?.some(hasUnsupportedDescendant) ?? false;
}

function rangesOverlap(left: SourceRange, right: SourceRange): boolean {
  return left.from < right.to && right.from < left.to;
}

function customUnsupportedRanges(source: string): SourceRange[] {
  const ranges: SourceRange[] = [];
  const lines = Array.from(source.matchAll(/[^\r\n]*(?:\r\n|\n|$)/g))
    .filter((match) => match[0].length > 0)
    .map((match) => ({
      from: match.index,
      to: match.index + match[0].length,
      text: match[0].replace(/\r?\n$/, ""),
    }));

  if (lines[0]?.text.trim() === "---") {
    const closingIndex = lines.findIndex(
      (line, index) => index > 0 && line.text.trim() === "---",
    );
    if (closingIndex > 0) {
      const closingLine = lines[closingIndex];
      if (closingLine) {
        ranges.push({ from: 0, to: closingLine.to });
      }
    }
  }

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (!line) {
      continue;
    }

    const trimmed = line.text.trim();
    if (/^\$\$.+\$\$$/.test(trimmed)) {
      ranges.push({ from: line.from, to: line.to });
      continue;
    }

    if (trimmed === "$$") {
      const closingIndex = lines.findIndex(
        (candidate, candidateIndex) =>
          candidateIndex > index && candidate.text.trim() === "$$",
      );
      const closingLine = closingIndex > index ? lines[closingIndex] : undefined;
      ranges.push({ from: line.from, to: closingLine?.to ?? line.to });
      if (closingLine) {
        index = closingIndex;
      }
      continue;
    }

    if (/(^|[^\\])\$[^$\r\n]+\$/.test(line.text)) {
      ranges.push({ from: line.from, to: line.to });
    }
  }

  return ranges;
}

function topLevelNodes(source: string): readonly PositionedNode[] {
  const tree = unified().use(remarkParse).use(remarkGfm).parse(source);
  return (tree as PositionedNode).children ?? [];
}

export function createMarkdownProjection(
  source: string,
  revision: number,
): MarkdownProjection {
  const customRanges = customUnsupportedRanges(source);
  const rawSegments = topLevelNodes(source).flatMap((node) => {
    const range = nodeRange(node);
    if (!range) {
      return [];
    }

    const sourceBacked =
      hasUnsupportedDescendant(node) ||
      customRanges.some((unsupported) => rangesOverlap(range, unsupported));

    return [
      {
        id: "",
        kind: sourceBacked ? "source-backed" : "visual",
        from: range.from,
        to: range.to,
        source: source.slice(range.from, range.to),
        nodeType: node.type,
      } satisfies MarkdownProjectionSegment,
    ];
  });

  const grouped = rawSegments.reduce<Array<Omit<MarkdownProjectionSegment, "id">>>(
    (segments, segment) => {
      const previous = segments.at(-1);
      if (previous?.kind === segment.kind) {
        segments[segments.length - 1] = {
          kind: previous.kind,
          from: previous.from,
          to: segment.to,
          source: source.slice(previous.from, segment.to),
          nodeType: previous.nodeType === segment.nodeType ? previous.nodeType : "blocks",
        };
      } else {
        segments.push({
          kind: segment.kind,
          from: segment.from,
          to: segment.to,
          source: segment.source,
          nodeType: segment.nodeType,
        });
      }
      return segments;
    },
    [],
  );
  const segments = grouped.map((segment, index) => ({
    ...segment,
    id: `${segment.kind}:${index}`,
  }));

  return { revision, segments };
}
