export type MarkdownLineEnding = "lf" | "crlf";

export interface DecodedMarkdown {
  readonly source: string;
  readonly isValidUtf8: boolean;
  readonly hasUtf8Bom: boolean;
  readonly lineEnding: MarkdownLineEnding;
  readonly originalBytes: Uint8Array;
}

const UTF8_BOM = Uint8Array.of(0xef, 0xbb, 0xbf);

function startsWithUtf8Bom(bytes: Uint8Array): boolean {
  return (
    bytes.length >= UTF8_BOM.length &&
    UTF8_BOM.every((value, index) => bytes[index] === value)
  );
}

function detectLineEnding(source: string): MarkdownLineEnding {
  const crlfCount = source.match(/\r\n/g)?.length ?? 0;
  const lfCount = (source.match(/\n/g)?.length ?? 0) - crlfCount;
  return crlfCount > lfCount ? "crlf" : "lf";
}

export function decodeMarkdownBytes(bytes: Uint8Array): DecodedMarkdown {
  const originalBytes = Uint8Array.from(bytes);
  const hasUtf8Bom = startsWithUtf8Bom(originalBytes);
  const contentBytes = hasUtf8Bom
    ? originalBytes.subarray(UTF8_BOM.length)
    : originalBytes;

  let source: string;
  let isValidUtf8 = true;

  try {
    source = new TextDecoder("utf-8", { fatal: true }).decode(contentBytes);
  } catch {
    source = new TextDecoder("utf-8").decode(contentBytes);
    isValidUtf8 = false;
  }

  return {
    source,
    isValidUtf8,
    hasUtf8Bom,
    lineEnding: detectLineEnding(source),
    originalBytes,
  };
}

export function encodeMarkdownSource(
  source: string,
  hasUtf8Bom: boolean,
): Uint8Array {
  const content = new TextEncoder().encode(source);
  if (!hasUtf8Bom) {
    return content;
  }

  const bytes = new Uint8Array(UTF8_BOM.length + content.length);
  bytes.set(UTF8_BOM);
  bytes.set(content, UTF8_BOM.length);
  return bytes;
}
